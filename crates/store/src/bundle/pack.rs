//! Pack
//!
//! Bundle writes over blob reads.

use std::collections::BTreeMap;
use std::path::Path;

use confit_driver::{self as driver};
use confit_model::document::BlobRef;
use confit_model::manifest::Manifest;
use confit_model::progress::ProgressSender;
use confit_model::sha::Sha;

use super::BundleStore;
use super::ensure_bundle_extension;
use super::error::{BundleError, Result};
use super::{BUNDLE_BLOBS_PREFIX, BUNDLE_MANIFEST};
use crate::blob::error::BlobError;
use crate::handles::ArchiveHandle;

/// Entry mode for bundle members.
const BUNDLE_ENTRY_MODE: u32 = 0o644;

impl BundleStore {
    /// Writes one portable bundle holding manifest and blobs.
    ///
    /// Bare destinations gain the bundle extension and the
    /// sealed handle names the written file. The manifest rides
    /// first with blob members sorted by stored hash.
    ///
    /// # Errors
    ///
    /// - [`BundleError::Unknown`] for manifest render and other write failures.
    /// - [`BundleError::Missing`] for missing blobs.
    /// - [`BundleError::Unreachable`] for unreachable archives.
    /// - [`BundleError::Denied`] for denied archives.
    pub fn write(
        &self,
        manifest: &Manifest,
        dest: &Path,
        progress: Option<&ProgressSender>,
    ) -> Result<ArchiveHandle> {
        let _ = progress;
        let dest = ensure_bundle_extension(dest);
        let text = manifest.json().map_err(|error| BundleError::Unknown {
            path: dest.clone(),
            message: error.to_string(),
        })?;
        let mut members = Vec::with_capacity(manifest.refs().len() + 1);
        members.push(driver::tar::BuildMember {
            name: BUNDLE_MANIFEST.to_owned(),
            len: text.len() as u64,
            reader: Box::new(std::io::Cursor::new(text.as_bytes())),
            mode: BUNDLE_ENTRY_MODE,
        });
        for blob in sorted_blobs(manifest) {
            let (name, len, reader) = self.stored_member(&blob, &dest)?;
            members.push(driver::tar::BuildMember {
                name,
                len,
                reader,
                mode: BUNDLE_ENTRY_MODE,
            });
        }
        seal_bundle(&dest, members)
    }

    /// Opens one blob source under its container member name.
    ///
    /// Reads ride the stored hash with encoded length, so pool
    /// bytes land in the tar untouched.
    ///
    /// # Errors
    ///
    /// - [`BundleError::Missing`] for missing blobs.
    /// - [`BundleError::Unknown`] for other blob failures.
    fn stored_member(
        &self,
        blob: &BlobRef,
        dest: &Path,
    ) -> Result<(String, u64, Box<dyn std::io::Read>)> {
        let handle = self.blobs.resolve(blob).map_err(|error| match error {
            BlobError::Missing { sha } => BundleError::Missing { sha },
            other => BundleError::Unknown {
                path: dest.to_path_buf(),
                message: other.to_string(),
            },
        })?;
        let (len, reader) = self
            .blobs
            .open_stored(&handle)
            .map_err(|error| match error {
                BlobError::Missing { sha } => BundleError::Missing { sha },
                other => BundleError::Unknown {
                    path: dest.to_path_buf(),
                    message: other.to_string(),
                },
            })?;
        let name = [BUNDLE_BLOBS_PREFIX, &blob.stored().to_string()].concat();
        Ok((name, len, reader))
    }
}

/// Lists one manifest's refs sorted by stored hash.
///
/// Duplicate stored hashes keep their first ref.
fn sorted_blobs(manifest: &Manifest) -> Vec<BlobRef> {
    let mut unique: BTreeMap<String, BlobRef> = BTreeMap::new();
    for blob in manifest.refs() {
        unique.entry(blob.stored().hex()).or_insert(blob);
    }
    unique.into_values().collect()
}

/// Seals one staged bundle into its destination file.
///
/// Member order reads as given, so callers own entry
/// order. The sealed handle carries the finished file
/// path and its content hash.
///
/// # Errors
///
/// - [`BundleError::Unreachable`] for missing destinations.
/// - [`BundleError::Denied`] for denied destinations.
/// - [`BundleError::Unknown`] for other build and hash failures.
fn seal_bundle(dest: &Path, members: Vec<driver::tar::BuildMember<'_>>) -> Result<ArchiveHandle> {
    driver::atomic_write(dest, |path| {
        let sink = driver::fs::create(path)?;
        driver::tar::build_plain(sink, members)
    })
    .map_err(|error| BundleError::from_io(dest, error))?;
    let sha = bundle_sha(dest)?;
    let sealed = ArchiveHandle::new(dest.to_path_buf(), sha);
    sealed.map_err(|error| BundleError::Unknown {
        path: dest.to_path_buf(),
        message: error.to_string(),
    })
}

/// Hashes one finished bundle file with a stream.
///
/// # Errors
///
/// - [`BundleError::Unreachable`] for missing bundles.
/// - [`BundleError::Denied`] for denied bundles.
/// - [`BundleError::Unknown`] for other failures.
fn bundle_sha(source: &Path) -> Result<Sha> {
    let mut file = driver::fs::open(source).map_err(|error| BundleError::from_io(source, error))?;
    Sha::read(&mut file).map_err(|error| BundleError::from_io(source, error))
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;
    use confit_model::document::{Data, Document};
    use confit_model::routes::{Route, RouteBase};
    use confit_model::sha::Sha;

    use crate::StoreRoots;
    use crate::archive::ArchiveStore;
    use crate::blob::{BlobSource, BlobStore};
    use crate::handles::TrustedHandle;
    use confit_driver::{self as driver, fs::TestGuard};

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            cache_base: dir.join("cache"),
            temp_base: dir.join("temp"),
        }
    }

    fn opaque_manifest(dir: &Path, bodies: &[&[u8]]) -> (BundleStore, Manifest) {
        use std::sync::Arc;

        use crate::resources::Resources;

        let roots = test_roots(dir);
        let resources = Arc::new(Resources::new(&roots));
        let archives = Arc::new(ArchiveStore::new(&roots, resources.clone()));
        let blobs = Arc::new(BlobStore::new(&roots));
        let mut documents = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            let handle = match blobs.put(BlobSource::Bytes(body)) {
                Ok(handle) => handle,
                Err(error) => panic!("cache stores: {error}"),
            };
            documents.push(Document::new(
                Route::new(RouteBase::Home, format!("bin-{index}").as_str()).unwrap(),
                Data::Opaque {
                    blob: handle.to_ref(),
                    size: body.len() as u64,
                    mode: None,
                    unmanaged: false,
                },
            ));
        }
        match Manifest::build(documents, Vec::new()) {
            Ok(manifest) => (BundleStore::new(archives, blobs, resources), manifest),
            Err(error) => panic!("manifest builds: {error}"),
        }
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let file = driver::fs::open(path).unwrap();
        let mut archive = tar::Archive::new(file);
        let mut names = Vec::new();
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            names.push(entry.path().unwrap().to_string_lossy().into_owned());
        }
        names
    }

    fn entry_bodies(path: &Path) -> Vec<Vec<u8>> {
        let file = driver::fs::open(path).unwrap();
        let mut archive = tar::Archive::new(file);
        let mut bodies = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let name = entry.path().unwrap().to_string_lossy().into_owned();
            if name == BUNDLE_MANIFEST {
                continue;
            }
            let mut body = Vec::new();
            entry.read_to_end(&mut body).unwrap();
            bodies.push(body);
        }
        bodies
    }

    #[test]
    fn write_sources_cache_with_empty_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"cache-only bytes"]);
        let roots = test_roots(dir.path());
        assert!(
            driver::fs::read_dir(&roots.config_base.join("blobs"))
                .unwrap_or_default()
                .is_empty(),
            "plan output sources the cache with zero pool bytes"
        );
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan, None) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        let names = entry_names(written.canonical());
        assert_eq!(names[0], BUNDLE_MANIFEST, "manifest rides first");
        assert_eq!(names.len(), 2, "the cache blob rides along");
    }

    #[test]
    fn write_seals_finished_file_with_streamed_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"leading bytes", b"streamed body"]);
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan, None) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        assert_eq!(
            written.canonical(),
            dir.path().join("plan.cb").as_path(),
            "seal points at the destination"
        );
        let raw = driver::fs::read(written.canonical()).unwrap();
        assert_eq!(
            written.sha(),
            &Sha::hash(&raw),
            "seal hashes the finished bytes"
        );
        let mut bodies = entry_bodies(written.canonical());
        bodies.sort();
        let mut expected = Vec::new();
        for blob in manifest.refs() {
            let handle = match store.blobs.resolve(&blob) {
                Ok(handle) => handle,
                Err(error) => panic!("blob resolves: {error}"),
            };
            let (_, mut reader) = match store.blobs.open_stored(&handle) {
                Ok(opened) => opened,
                Err(error) => panic!("blob opens: {error}"),
            };
            let mut body = Vec::new();
            reader.read_to_end(&mut body).unwrap();
            expected.push(body);
        }
        expected.sort();
        assert_eq!(bodies, expected, "streamed blobs ride intact");
    }

    #[test]
    fn write_falls_back_to_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"pool bytes"]);
        let roots = test_roots(dir.path());
        let pools = BlobStore::new(&roots);
        let mut handles = Vec::new();
        for blob in manifest.refs() {
            match pools.resolve(&blob) {
                Ok(handle) => handles.push(handle),
                Err(error) => panic!("persist proves: {error}"),
            }
        }
        match pools.persist(&handles) {
            Ok(_) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        let cached = match driver::fs::read_dir(&roots.cache_base.join("blobs")) {
            Ok(cached) => cached,
            Err(error) => panic!("cache lists: {error}"),
        };
        for path in cached {
            driver::fs::remove_file(&path).unwrap();
        }
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan, None) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        let names = entry_names(written.canonical());
        assert_eq!(names[0], BUNDLE_MANIFEST, "manifest rides first");
        assert_eq!(names.len(), 2, "the pool blob rides along");
    }
}
