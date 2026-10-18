//! Bundle output.

use std::collections::BTreeSet;
use std::path::Path;

use confit_driver as driver;
use confit_driver::sha::Sha;
use confit_store::blob::error::BlobError;
use confit_store::faults::AccessFault;
use confit_store::handles::ArchiveHandle;

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
    /// The verified handle is the written file.
    ///
    /// - Destinations carry the bundle extension.
    /// - Members sort manifest-first in content-hash order.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for unreadable bundles.
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
        let blobs = sorted_blobs(manifest);
        self.write_staged(&dest, &text, &blobs)
    }

    /// Open one blob reader with its member name and length.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleMissing`] for missing blobs.
    /// - [`RuntimeError::BundleRead`] for other unreadable blobs.
    fn object_member(&self, blob: &Sha, dest: &Path) -> Result<(String, u64, std::fs::File)> {
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
            .open_object(&handle)
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
        let name = [BUNDLE_BLOBS_PREFIX, &blob.to_string()].concat();
        Ok((name, len, reader))
    }

    /// Write one staged bundle into its destination file.
    ///
    /// Verified handle with the finished file path and its content hash.
    ///
    /// - Member order as given.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for unreadable bundles.
    /// - [`RuntimeError::BundleWrite`] for write faults.
    /// - [`RuntimeError::BundleWriteUnknown`] for other build and hash
    ///   failures.
    fn write_staged(&self, dest: &Path, text: &str, blobs: &[Sha]) -> Result<ArchiveHandle> {
        let mut staged = Vec::with_capacity(blobs.len());
        for blob in blobs {
            staged.push(self.object_member(blob, dest)?);
        }
        driver::atomic_write(dest, |path| {
            let sink = std::fs::File::create(path)?;
            let mut builder = driver::tar::TarBuilder::create(sink);
            let mut manifest_reader = std::io::Cursor::new(text.as_bytes());
            builder.member(
                BUNDLE_MANIFEST,
                text.len() as u64,
                BUNDLE_ENTRY_MODE,
                &mut manifest_reader,
            )?;
            for (name, len, mut reader) in staged {
                builder.member(&name, len, BUNDLE_ENTRY_MODE, &mut reader)?;
            }
            builder.finish()?;
            Ok(())
        })
        .map_err(|error| RuntimeError::from_bundle_write_io(dest, error))?;
        self.archives
            .register_archive(dest)
            .map_err(|error| RuntimeError::from_archive(dest, error))
    }
}

/// List one manifest's hashes sorted by content hash.
///
/// - Duplicate content hashes collapse into one entry.
fn sorted_blobs(manifest: &Manifest) -> Vec<Sha> {
    let mut unique: BTreeSet<Sha> = BTreeSet::new();
    for blob in manifest.refs() {
        unique.insert(blob);
    }
    unique.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;

    use super::*;
    use confit_driver as driver;
    use confit_driver::sha::Sha;
    use confit_model::document::{Data, Document, OpaqueDocument, Payload};
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

        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots, sender);
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut documents = Vec::new();
        for (index, body) in bodies.iter().enumerate() {
            let name = format!("bin-{index}.bin");
            std::fs::write(project.join(&name), body).unwrap();
            let source = match stores.resources().resource(&name) {
                Ok(handle) => handle,
                Err(error) => panic!("project opens: {error}"),
            };
            let handle = match stores.blobs().put(&source) {
                Ok(handle) => handle,
                Err(error) => panic!("cache stores: {error}"),
            };
            documents.push(Document::new(
                Route::new(RouteBase::Home, format!("bin-{index}").as_str()).unwrap(),
                Data::Opaque(OpaqueDocument {
                    handle: Payload::Resolved(handle.sha256().clone()),
                    mode: 0o644,
                    unmanaged: false,
                }),
            ));
        }
        match Manifest::build(&stores, documents, Vec::new()) {
            Ok(manifest) => (Bundler::new(&stores), manifest),
            Err(error) => panic!("manifest builds: {error}"),
        }
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        let mut archive = tar::Archive::new(file);
        let mut names = Vec::new();
        for entry in archive.entries().unwrap() {
            let entry = entry.unwrap();
            names.push(entry.path().unwrap().to_string_lossy().into_owned());
        }
        names
    }

    fn entry_bodies(path: &Path) -> Vec<Vec<u8>> {
        let file = std::fs::File::open(path).unwrap();
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
        let raw = std::fs::read(written.canonical()).unwrap();
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
            let (_, mut reader) = match store.blobs.open_object(&handle) {
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
            std::fs::remove_file(&path).unwrap();
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

    #[cfg(unix)]
    #[test]
    fn write_reports_write_denied() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"written bytes"]);
        let locked = dir.path().join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        driver::fs::set_mode(&locked, Some(0o555)).unwrap();
        let dest = locked.join("plan");
        match store.write(&manifest, &dest) {
            Ok(_) => panic!("denied disk passes"),
            Err(RuntimeError::BundleWrite { path, fault }) => {
                assert!(
                    matches!(fault, AccessFault::Denied),
                    "write keeps the fault"
                );
                assert_eq!(path, locked.join("plan.cb"), "write keeps the destination");
            }
            Err(error) => panic!("wrong write variant: {error}"),
        }
        driver::fs::set_mode(&locked, Some(0o755)).unwrap();
    }
}
