//! Unpack
//!
//! Bundle reads over archive extraction and blob receives.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use confit_model::manifest::Manifest;
use confit_model::sha::Sha;

use super::codec::{CodecError, check_blob_id, decode};
use super::error::{BundleError, Result, from_archive, from_resource};
use super::{BUNDLE_BLOBS_PREFIX, BUNDLE_MANIFEST, BundleStore};
use crate::blob::error::BlobError;
use crate::handles::ArchiveHandle;

impl BundleStore {
    /// Reads one portable bundle into a live manifest.
    ///
    /// # Errors
    ///
    /// - [`BundleError::Read`] for failed bundle reads.
    /// - [`BundleError::Version`] for unsupported versions.
    /// - [`BundleError::Missing`] for missing blobs.
    pub fn read(&self, path: &Path) -> Result<Manifest> {
        let handle = self
            .archives
            .seal(path)
            .map_err(|error| from_archive(path, error))?;
        let names = self
            .archives
            .members(&handle)
            .map_err(|error| from_archive(path, error))?;
        let (has_manifest, wanted) = check_names(&names, path)?;
        if !has_manifest {
            return Err(unknown(path, "missing manifest".to_owned()));
        }
        let manifest = self.read_manifest(&handle, path)?;
        let refs = manifest.refs();
        for blob in &refs {
            if !wanted
                .iter()
                .any(|name| name == blob.stored().hex().as_str())
            {
                return Err(BundleError::Missing {
                    sha: blob.sha().clone(),
                });
            }
        }
        for stored in &wanted {
            self.receive_blob(&handle, stored, path)?;
        }
        Ok(manifest)
    }

    /// Reads and parses the bundle manifest member.
    ///
    /// # Errors
    ///
    /// - [`BundleError::Read`] for failed member reads.
    /// - [`BundleError::Version`] for stale versions.
    fn read_manifest(&self, handle: &ArchiveHandle, bundle: &Path) -> Result<Manifest> {
        let member = self
            .archives
            .extract_member(handle, BUNDLE_MANIFEST)
            .map_err(|error| from_archive(bundle, error))?;
        let mut reader = self
            .resources
            .open(&member)
            .map_err(|error| from_resource(bundle, error))?;
        let mut raw = Vec::new();
        reader
            .read_to_end(&mut raw)
            .map_err(|error| BundleError::from_io(bundle, error))?;
        decode(&raw).map_err(|error| from_codec(bundle, error))
    }

    /// Receives one blob member into the cache blind.
    ///
    /// # Errors
    ///
    /// - [`BundleError::Read`] for failed member reads.
    /// - [`BundleError::Write`] for receive write faults.
    /// - [`BundleError::WriteUnknown`] for other receive write failures.
    fn receive_blob(&self, handle: &ArchiveHandle, stored: &str, bundle: &Path) -> Result<()> {
        let name = [BUNDLE_BLOBS_PREFIX, stored].concat();
        let member = self
            .archives
            .extract_member(handle, &name)
            .map_err(|error| from_archive(bundle, error))?;
        let stored_sha = Sha::new(stored).map_err(|error| unknown(bundle, error.to_string()))?;
        self.blobs
            .receive(&stored_sha, &member)
            .map_err(|error| from_blob(bundle, error))?;
        Ok(())
    }
}

/// Validates bundle member names with verbatim policy.
///
/// # Errors
///
/// - [`BundleError::Read`] for failed bundle reads.
/// - [`BundleError::Version`] for stale codec versions.
fn check_names(names: &[String], bundle: &Path) -> Result<(bool, Vec<String>)> {
    let mut has_manifest = false;
    let mut wanted: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for name in names {
        if name == BUNDLE_MANIFEST {
            if has_manifest {
                return Err(unknown(bundle, "duplicate manifest".to_owned()));
            }
            has_manifest = true;
        } else if let Some(stored) = name.strip_prefix(BUNDLE_BLOBS_PREFIX) {
            check_blob_id(stored).map_err(|error| from_codec(bundle, error))?;
            if !seen.insert(stored.to_string()) {
                return Err(unknown(bundle, "duplicate blob".to_owned()));
            }
            wanted.push(stored.to_string());
        } else {
            return Err(unknown(bundle, "unexpected entry".to_owned()));
        }
    }
    Ok((has_manifest, wanted))
}

/// Builds one bundle read failure naming the bundle.
fn unknown(bundle: &Path, message: String) -> BundleError {
    BundleError::Read {
        path: bundle.to_path_buf(),
        fault: crate::faults::AccessFault::Unknown { message },
    }
}

/// Maps one codec failure at the bundle path into bundle language.
fn from_codec(bundle: &Path, error: CodecError) -> BundleError {
    match error {
        CodecError::Version { got } => BundleError::Version {
            path: bundle.to_path_buf(),
            got,
        },
        CodecError::Corrupt { message } => unknown(bundle, message),
    }
}

/// Maps one blob failure at the bundle path into bundle language.
fn from_blob(bundle: &Path, error: BlobError) -> BundleError {
    use crate::faults::AccessFault;

    match error {
        BlobError::Read {
            sha,
            fault: AccessFault::Missing,
        } => BundleError::Missing { sha },
        BlobError::Read { fault, .. } => BundleError::Read {
            path: bundle.to_path_buf(),
            fault,
        },
        BlobError::Write { fault, .. } => BundleError::Write {
            path: bundle.to_path_buf(),
            fault,
        },
        BlobError::WriteUnknown { message, .. } => BundleError::WriteUnknown {
            path: bundle.to_path_buf(),
            message,
        },
        other => unknown(bundle, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{Data, Document};
    use confit_model::routes::{Route, RouteBase};

    use crate::StoreRoots;
    use crate::archive::ArchiveStore;
    use crate::blob::{BlobSource, BlobStore};
    use crate::handles::TrustedHandle;
    use confit_driver as driver;
    use confit_driver::fs::TestGuard;

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
        let (sender, _) = crossbeam_channel::unbounded();
        let archives = Arc::new(ArchiveStore::new(&roots, resources.clone(), sender.clone()));
        let blobs = Arc::new(BlobStore::new(&roots, sender));
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

    fn build_bundle(dest: &Path, entries: &[(&str, Vec<u8>)]) {
        let build = |path: &Path| {
            let members: Vec<driver::tar::BuildMember<'_>> = entries
                .iter()
                .map(|(name, bytes)| driver::tar::BuildMember {
                    name: (*name).to_owned(),
                    len: bytes.len() as u64,
                    reader: Box::new(std::io::Cursor::new(bytes.as_slice())),
                    mode: 0o644,
                })
                .collect();
            let sink = driver::fs::create(path)?;
            driver::tar::build_plain(sink, members)
        };
        if let Err(error) = driver::atomic_write(dest, build) {
            panic!("fixture bundle builds: {error}");
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

    #[test]
    fn round_trip_writes_manifest_first_with_sorted_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"alpha", b"beta"]);
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        assert_eq!(
            written.canonical(),
            dir.path().join("plan.cb"),
            "bare output gains suffix"
        );
        let names = entry_names(written.canonical());
        assert_eq!(names[0], BUNDLE_MANIFEST, "manifest rides first");
        let blobs: Vec<String> = names[1..].to_vec();
        assert_eq!(blobs.len(), 2, "every blob rides along");
        let mut sorted = blobs.clone();
        sorted.sort();
        assert_eq!(blobs, sorted, "blob entries ride sorted");
        for name in &blobs {
            assert!(
                name.starts_with(BUNDLE_BLOBS_PREFIX),
                "blob entry rides under prefix: {name}"
            );
        }
        match store.read(written.canonical()) {
            Ok(found) => {
                assert_eq!(found, manifest, "manifest round-trips");
            }
            Err(error) => panic!("manifest reads: {error}"),
        }
    }

    #[test]
    fn read_refuses_stale_version() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, _) = opaque_manifest(dir.path(), &[]);
        let stale = serde_json::json!({"version": 0u32, "documents": []});
        let raw = serde_json::to_vec(&stale).unwrap();
        let path = dir.path().join("stale.cb");
        build_bundle(&path, &[(BUNDLE_MANIFEST, raw)]);
        match store.read(&path) {
            Ok(_) => panic!("stale manifest passes"),
            Err(BundleError::Version { path: found, got }) => {
                assert_eq!(found, path, "stale keeps its path");
                assert_eq!(got, 0, "stale keeps its version");
            }
            Err(error) => panic!("wrong stale variant: {error}"),
        }
    }

    #[test]
    fn read_refuses_missing_blob() {
        use std::sync::Arc;

        use crate::resources::Resources;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let resources = Arc::new(Resources::new(&roots));
        let (sender, _) = crossbeam_channel::unbounded();
        let archives = Arc::new(ArchiveStore::new(&roots, resources.clone(), sender.clone()));
        let blobs = Arc::new(BlobStore::new(&roots, sender));
        let store = BundleStore::new(archives, blobs.clone(), resources);
        let handle = match blobs.put(BlobSource::Bytes(b"wanted bytes")) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores: {error}"),
        };
        let manifest = match Manifest::build(
            vec![Document::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                Data::Opaque {
                    blob: handle.to_ref(),
                    size: 12,
                    mode: None,
                    unmanaged: false,
                },
            )],
            Vec::new(),
        ) {
            Ok(manifest) => manifest,
            Err(error) => panic!("manifest builds: {error}"),
        };
        let raw = serde_json::to_vec(&manifest).unwrap();
        let path = dir.path().join("thin.cb");
        build_bundle(&path, &[(BUNDLE_MANIFEST, raw)]);
        match store.read(&path) {
            Ok(_) => panic!("thin manifest passes"),
            Err(error) => assert!(
                error.to_string().contains(&handle.sha().hex()),
                "missing blob names the hash: {error}"
            ),
        }
    }

    #[test]
    fn read_rejects_duplicate_and_unexpected_and_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, _) = opaque_manifest(dir.path(), &[]);

        let manifest = serde_json::to_vec(&Manifest::empty()).unwrap();
        let dup_path = dir.path().join("dup-manifest.cb");
        build_bundle(
            &dup_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                (BUNDLE_MANIFEST, manifest.clone()),
            ],
        );
        match store.read(&dup_path) {
            Ok(_) => panic!("duplicate manifest passes"),
            Err(error) => assert!(
                error.to_string().contains("duplicate manifest"),
                "duplicate reports itself: {error}"
            ),
        }

        let bad_path = dir.path().join("bad-id.cb");
        build_bundle(
            &bad_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                ("blobs/short", b"x".to_vec()),
            ],
        );
        match store.read(&bad_path) {
            Ok(_) => panic!("bad id passes"),
            Err(error) => assert!(
                error.to_string().contains("bad blob"),
                "bad id reports itself: {error}"
            ),
        }

        let odd_path = dir.path().join("odd.cb");
        build_bundle(
            &odd_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                ("other.txt", b"x".to_vec()),
            ],
        );
        match store.read(&odd_path) {
            Ok(_) => panic!("unexpected entry passes"),
            Err(error) => assert!(
                error.to_string().contains("unexpected entry"),
                "unexpected reports itself: {error}"
            ),
        }
    }

    #[test]
    fn write_seals_plain_tar_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"legacy bytes"]);
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        let raw = driver::fs::read(written.canonical()).unwrap();
        assert!(raw.len() >= 512, "plain holds one block");
        assert_eq!(&raw[257..262], b"ustar", "plain envelope carries tar magic");
    }
}
