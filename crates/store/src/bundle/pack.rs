//! Pack
//!
//! Bundle writes over blob reads.

use std::path::{Path, PathBuf};

use confit_driver as driver;
use confit_model::document::BlobRef;
use confit_model::error::{Error, Result};
use confit_model::progress::ProgressSender;

use super::{BUNDLE_BLOBS_PREFIX, BUNDLE_GZIP_LEVEL, BUNDLE_MANIFEST, BundleStore};
use super::{Bundle, ensure_bundle_extension};
use crate::blob::BlobStore;

/// Staging suffix for atomic bundle writes.
const STAGING_SUFFIX: &str = ".part";

/// Entry mode for bundle members.
const BUNDLE_ENTRY_MODE: u32 = 0o644;

impl BundleStore {
    /// Writes one portable bundle holding manifest and blobs.
    ///
    /// Bare destinations gain the bundle extension and the
    /// returned path names the written file.
    ///
    /// # Errors
    ///
    /// Compression and write failures surface as plan errors.
    pub fn write(
        &self,
        bundle: &Bundle,
        dest: &Path,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf> {
        let _ = progress;
        let dest = ensure_bundle_extension(dest);
        let manifest = serde_json::to_vec_pretty(&bundle.manifest)
            .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
        if let Some(parent) = dest.parent() {
            driver::create_dir_all(parent).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        }
        let mut staging = dest.as_os_str().to_owned();
        staging.push(STAGING_SUFFIX);
        let staging = PathBuf::from(staging);
        let out = driver::create(&staging)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        let encoder =
            flate2::write::GzEncoder::new(out, flate2::Compression::new(BUNDLE_GZIP_LEVEL));
        let mut builder = tar::Builder::new(encoder);
        append_manifest(&mut builder, &manifest, &dest)?;
        let mut refs: Vec<&BlobRef> = bundle.blobs.values().collect();
        refs.sort_by_key(|left| left.stored().hex());
        for blob in refs {
            append_blob(&mut builder, blob, &self.blobs, &dest)?;
        }
        let encoder = builder
            .into_inner()
            .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
        encoder
            .finish()
            .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
        driver::rename(&staging, &dest)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        Ok(dest)
    }
}

/// Appends one file entry to a bundle archive.
///
/// # Errors
///
/// Archive failures surface as plan errors naming the bundle.
fn append_manifest(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Box<dyn std::io::Write>>>,
    bytes: &[u8],
    dest: &Path,
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(BUNDLE_ENTRY_MODE);
    header.set_cksum();
    builder
        .append_data(&mut header, BUNDLE_MANIFEST, bytes)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))
}

/// Appends one blob entry with verbatim stored bytes.
///
/// Reads ride the stored handle with encoded length, so pool
/// bytes land in the tar untouched.
///
/// # Errors
///
/// Missing and unreadable blobs fail as plan errors naming
/// the bundle.
fn append_blob(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Box<dyn std::io::Write>>>,
    blob: &BlobRef,
    blobs: &BlobStore,
    dest: &Path,
) -> Result<()> {
    let handle = blobs.resolve(blob).map_err(|_| {
        Error::Plan(format!(
            "render bundle '{}': missing blob '{}'",
            dest.display(),
            blob.stored()
        ))
    })?;
    let (len, source) = blobs
        .open_stored(&handle)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    let mut header = tar::Header::new_gnu();
    header.set_size(len);
    header.set_mode(BUNDLE_ENTRY_MODE);
    header.set_cksum();
    builder
        .append_data(
            &mut header,
            format!("{BUNDLE_BLOBS_PREFIX}{}", blob.stored()),
            source,
        )
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{ManifestData, ManifestDocument};
    use confit_model::routes::{Route, RouteBase};

    use crate::StoreRoots;
    use crate::archive::ArchiveStore;
    use crate::blob::{BlobSource, BlobStore};
    use confit_driver::TestGuard;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            cache_base: dir.join("cache"),
            temp_base: dir.join("temp"),
        }
    }

    fn opaque_bundle(dir: &Path, bodies: &[&[u8]]) -> (BundleStore, Bundle) {
        use std::sync::Arc;

        let roots = test_roots(dir);
        let archives = Arc::new(ArchiveStore::new(&roots));
        let blobs = Arc::new(BlobStore::new(&roots));
        let mut documents = Vec::new();
        let mut bundle = Bundle::empty();
        for (index, body) in bodies.iter().enumerate() {
            let handle = match blobs.put(BlobSource::Bytes(body)) {
                Ok(handle) => handle,
                Err(error) => panic!("cache stores: {error}"),
            };
            documents.push(ManifestDocument::new(
                Route::new(RouteBase::Home, format!("bin-{index}").as_str()).unwrap(),
                ManifestData::Opaque {
                    blob: handle.to_ref(),
                    size: body.len() as u64,
                    mode: None,
                    unmanaged: false,
                },
            ));
            bundle.blobs.insert(handle.sha().hex(), handle.to_ref());
        }
        match Bundle::build(documents, Vec::new()) {
            Ok(built) => {
                bundle.manifest = built.manifest;
                (BundleStore::new(archives, blobs), bundle)
            }
            Err(error) => panic!("bundle builds: {error}"),
        }
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let file = driver::open_read(path).unwrap();
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
        let mut names = Vec::new();
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            names.push(entry.path().unwrap().to_string_lossy().into_owned());
        }
        names
    }

    #[test]
    fn write_sources_cache_with_empty_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, bundle) = opaque_bundle(dir.path(), &[b"cache-only bytes"]);
        let roots = test_roots(dir.path());
        assert!(
            driver::read_dir(&roots.config_base.join("blobs"))
                .unwrap_or_default()
                .is_empty(),
            "plan output sources the cache with zero pool bytes"
        );
        let dest = match store.write(&bundle, &dir.path().join("plan"), None) {
            Ok(dest) => dest,
            Err(error) => panic!("bundle writes: {error}"),
        };
        let names = entry_names(&dest);
        assert_eq!(names[0], BUNDLE_MANIFEST, "manifest rides first");
        assert_eq!(names.len(), 2, "the cache blob rides along");
    }

    #[test]
    fn write_falls_back_to_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, bundle) = opaque_bundle(dir.path(), &[b"pool bytes"]);
        let roots = test_roots(dir.path());
        let pools = BlobStore::new(&roots);
        let mut handles = Vec::new();
        for blob in bundle.blobs.values() {
            match pools.resolve(blob) {
                Ok(handle) => handles.push(handle),
                Err(error) => panic!("persist proves: {error}"),
            }
        }
        match pools.persist(&handles) {
            Ok(_) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        let cached = match driver::read_dir(&roots.cache_base.join("blobs")) {
            Ok(cached) => cached,
            Err(error) => panic!("cache lists: {error}"),
        };
        for path in cached {
            driver::remove_file(&path).unwrap();
        }
        let dest = match store.write(&bundle, &dir.path().join("plan"), None) {
            Ok(dest) => dest,
            Err(error) => panic!("bundle writes: {error}"),
        };
        let names = entry_names(&dest);
        assert_eq!(names[0], BUNDLE_MANIFEST, "manifest rides first");
        assert_eq!(names.len(), 2, "the pool blob rides along");
    }
}
