//! Bundle output.

use std::collections::BTreeMap;
use std::path::Path;

use confit_driver::{self as driver};
use confit_store::blob::error::BlobError;
use confit_store::faults::AccessFault;
use confit_store::handles::ArchiveHandle;
use confit_store::handles::BlobRef;

use super::Bundler;
use super::ensure_bundle_extension;
use super::{BUNDLE_BLOBS_PREFIX, BUNDLE_MANIFEST};
use crate::error::{Result, RuntimeError};
use crate::manifest::Manifest;

/// Entry mode for bundle members.
const BUNDLE_ENTRY_MODE: u32 = 0o644;

impl Bundler {
    /// Write one portable bundle with the manifest and the blobs.
    ///
    /// The verified handle shows the written file.
    ///
    /// - Destinations carry the bundle extension.
    /// - Members sort manifest-first in stored-hash order.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for failed bundle reads.
    /// - [`RuntimeError::BundleMissing`] for missing blobs.
    /// - [`RuntimeError::BundleWrite`] for write faults.
    /// - [`RuntimeError::BundleWriteUnknown`] for other write
    ///   failures.
    /// - [`RuntimeError::BundleManifest`] for manifest render failures.
    pub fn write(&self, manifest: &Manifest, dest: &Path) -> Result<ArchiveHandle> {
        let dest = ensure_bundle_extension(dest);
        let text = manifest
            .json()
            .map_err(|error| RuntimeError::BundleManifest {
                path: dest.clone(),
                source: error,
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
        self.write_staged(&dest, members)
    }

    /// Open one blob reader with its member name and length.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleMissing`] for missing blobs.
    /// - [`RuntimeError::BundleRead`] for other failed blob reads.
    fn stored_member(
        &self,
        blob: &BlobRef,
        dest: &Path,
    ) -> Result<(String, u64, Box<dyn std::io::Read>)> {
        let handle = self.blobs.resolve(blob).map_err(|error| match error {
            BlobError::Read {
                sha,
                fault: AccessFault::Missing,
            } => RuntimeError::BundleMissing { sha },
            other => RuntimeError::BundleRead {
                path: dest.to_path_buf(),
                fault: AccessFault::Unknown {
                    message: other.to_string(),
                },
            },
        })?;
        let (len, reader) = self
            .blobs
            .open_stored(&handle)
            .map_err(|error| match error {
                BlobError::Read {
                    sha,
                    fault: AccessFault::Missing,
                } => RuntimeError::BundleMissing { sha },
                other => RuntimeError::BundleRead {
                    path: dest.to_path_buf(),
                    fault: AccessFault::Unknown {
                        message: other.to_string(),
                    },
                },
            })?;
        let name = [BUNDLE_BLOBS_PREFIX, &blob.stored.to_string()].concat();
        Ok((name, len, reader))
    }

    /// Write one staged bundle into its destination file.
    ///
    /// The verified handle carries the finished file path and its content hash.
    ///
    /// - Member order is as given.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for failed bundle reads.
    /// - [`RuntimeError::BundleWrite`] for write faults.
    /// - [`RuntimeError::BundleWriteUnknown`] for other build and hash
    ///   failures.
    fn write_staged(
        &self,
        dest: &Path,
        members: Vec<driver::tar::BuildMember<'_>>,
    ) -> Result<ArchiveHandle> {
        driver::atomic_write(dest, |path| {
            let sink = driver::fs::create(path)?;
            driver::tar::build_plain(sink, members)
        })
        .map_err(|error| RuntimeError::from_bundle_write_io(dest, error))?;
        self.archives
            .register_archive(dest)
            .map_err(|error| RuntimeError::from_archive(dest, error))
    }
}

/// List one manifest's refs sorted by stored hash.
///
/// - Duplicate stored hashes keep their first ref.
fn sorted_blobs(manifest: &Manifest) -> Vec<BlobRef> {
    let mut unique: BTreeMap<String, BlobRef> = BTreeMap::new();
    for blob in manifest.refs() {
        unique.entry(blob.stored.hex()).or_insert(blob);
    }
    unique.into_values().collect()
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;
    use confit_driver::sha::Sha;
    use confit_driver::{self as driver, fs::TestGuard};
    use confit_model::document::Data;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::StoreRoots;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        }
    }

    fn opaque_manifest(dir: &Path, bodies: &[&[u8]]) -> (Bundler, Manifest) {
        use confit_store::Stores;
        use confit_store::blob::BlobSource;

        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots, sender);
        let mut documents = BTreeMap::new();
        for (index, body) in bodies.iter().enumerate() {
            let handle = match stores.blobs().put(BlobSource::Bytes(body)) {
                Ok(handle) => handle,
                Err(error) => panic!("cache stores: {error}"),
            };
            documents.insert(
                Route::new(RouteBase::Home, format!("bin-{index}").as_str()).unwrap(),
                Data::Opaque {
                    blob: handle.to_ref(),
                    size: body.len() as u64,
                    mode: 0o644,
                    unmanaged: false,
                },
            );
        }
        match Manifest::build(documents, Vec::new()) {
            Ok(manifest) => (Bundler::new(&stores), manifest),
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
        let written = match store.write(&manifest, &plan) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        let names = entry_names(written.canonical());
        assert_eq!(names[0], BUNDLE_MANIFEST, "the manifest comes first");
        assert_eq!(names.len(), 2, "the cache blob is included");
    }

    #[test]
    fn write_registers_finished_file_with_streamed_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"leading bytes", b"streamed body"]);
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        assert_eq!(
            written.canonical(),
            dir.path().join("plan.cb").as_path(),
            "write points at the destination"
        );
        let raw = driver::fs::read(written.canonical()).unwrap();
        assert_eq!(
            written.sha256(),
            &Sha::hash(&raw),
            "write hashes the finished bytes"
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
        assert_eq!(bodies, expected, "streamed blobs stay intact");
    }

    #[test]
    fn write_falls_back_to_pool() {
        use confit_store::blob::BlobStore;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"pool bytes"]);
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let pools = BlobStore::new(&roots, sender);
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
        let cached = match driver::fs::read_dir(&roots.temp_base.join("blobs")) {
            Ok(cached) => cached,
            Err(error) => panic!("cache lists: {error}"),
        };
        for path in cached {
            driver::fs::remove_file(&path).unwrap();
        }
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        let names = entry_names(written.canonical());
        assert_eq!(names[0], BUNDLE_MANIFEST, "the manifest comes first");
        assert_eq!(names.len(), 2, "the pool blob is included");
    }

    #[test]
    fn write_reports_write_storage_full() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"written bytes"]);
        guard.fail_writes(std::io::ErrorKind::StorageFull);
        let dest = dir.path().join("plan");
        match store.write(&manifest, &dest) {
            Ok(_) => panic!("full disk passes"),
            Err(RuntimeError::BundleWrite { path, fault }) => {
                assert!(
                    matches!(fault, confit_store::faults::AccessFault::StorageFull),
                    "write keeps the fault"
                );
                assert_eq!(
                    path,
                    dir.path().join("plan.cb"),
                    "write keeps the destination"
                );
            }
            Err(error) => panic!("wrong write variant: {error}"),
        }
    }
}
