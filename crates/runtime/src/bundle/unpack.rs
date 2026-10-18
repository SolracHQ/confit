//! Bundle input.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::Path;

use confit_driver::sha::Sha;
use confit_model::document::Data;
use confit_store::blob::error::BlobError;
use confit_store::faults::AccessFault;
use confit_store::handles::ArchiveHandle;

use super::codec::{CodecError, check_blob_id, decode};
use super::{BUNDLE_BLOBS_PREFIX, BUNDLE_MANIFEST, Bundler};
use crate::error::{Result, RuntimeError};
use crate::manifest::Manifest;

impl Bundler {
    /// Read one portable bundle into a sealed manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for unreadable bundles.
    /// - [`RuntimeError::BundleVersion`] for unsupported versions.
    /// - [`RuntimeError::BundleMissing`] for missing blobs.
    pub fn read(&self, path: &Path) -> Result<Manifest> {
        let handle = self
            .archives
            .register_archive(path)
            .map_err(|error| RuntimeError::from_archive(path, error))?;
        let names = self
            .archives
            .members(&handle)
            .map_err(|error| RuntimeError::from_archive(path, error))?;
        let (has_manifest, wanted) = check_names(&names, path)?;
        if !has_manifest {
            return Err(unknown(path, "missing manifest".to_owned()));
        }
        let manifest = self.read_manifest(&handle, path)?;
        let refs = manifest.refs();
        for blob in &refs {
            if !wanted.iter().any(|name| name == blob.hex().as_str()) {
                return Err(RuntimeError::BundleMissing { sha: blob.clone() });
            }
        }
        for content in &wanted {
            self.receive_blob(&handle, content, path, &manifest)?;
        }
        Ok(manifest)
    }

    /// Read and parse the bundle manifest member.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleRead`] for unreadable members.
    /// - [`RuntimeError::BundleVersion`] for stale versions.
    fn read_manifest(&self, handle: &ArchiveHandle, bundle: &Path) -> Result<Manifest> {
        let member = self
            .archives
            .extract_member(handle, BUNDLE_MANIFEST)
            .map_err(|error| RuntimeError::from_archive(bundle, error))?;
        let mut reader = self
            .resources
            .open(&member)
            .map_err(|error| RuntimeError::from_resource(bundle, error))?;
        let mut raw = Vec::new();
        reader
            .read_to_end(&mut raw)
            .map_err(|error| RuntimeError::from_bundle_io(bundle, error))?;
        decode(&raw).map_err(|error| from_codec(bundle, error))
    }

    /// Receive one blob member into the cache.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BundleCorrupt`] for corrupt members.
    /// - [`RuntimeError::BundleRead`] for unreadable members.
    /// - [`RuntimeError::BundleWrite`] for receive write faults.
    /// - [`RuntimeError::BundleWriteUnknown`] for other receive write
    ///   failures.
    fn receive_blob(
        &self,
        handle: &ArchiveHandle,
        content: &str,
        bundle: &Path,
        manifest: &Manifest,
    ) -> Result<()> {
        let name = [BUNDLE_BLOBS_PREFIX, content].concat();
        let member = self
            .archives
            .extract_member(handle, &name)
            .map_err(|error| RuntimeError::from_archive(bundle, error))?;
        let content_sha = Sha::new(content).map_err(|error| unknown(bundle, error.to_string()))?;
        self.blobs
            .receive(&content_sha, &member)
            .map_err(|error| from_blob(bundle, manifest, &content_sha, error))?;
        Ok(())
    }
}

/// Validate bundle member names.
///
/// # Errors
///
/// - [`RuntimeError::BundleRead`] for unreadable bundles.
/// - [`RuntimeError::BundleVersion`] for stale codec versions.
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
        } else if let Some(content) = name.strip_prefix(BUNDLE_BLOBS_PREFIX) {
            check_blob_id(content).map_err(|error| from_codec(bundle, error))?;
            if !seen.insert(content.to_string()) {
                return Err(unknown(bundle, "duplicate blob".to_owned()));
            }
            wanted.push(content.to_string());
        } else {
            return Err(unknown(bundle, "unexpected entry".to_owned()));
        }
    }
    Ok((has_manifest, wanted))
}

/// List manifest documents holding one content hash.
fn holders(manifest: &Manifest, sha: &Sha) -> String {
    let found: Vec<String> = manifest
        .documents
        .iter()
        .filter(|document| match &document.data {
            Data::Opaque(found) => found.handle.resolved() == Some(sha),
            Data::Tree(found) => found
                .members
                .values()
                .filter_map(|member| member.blob().resolved())
                .any(|blob| blob == sha),
            Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => false,
        })
        .map(|document| format!("'{}'", document.destination.display()))
        .collect();
    if found.is_empty() {
        "an unlisted bundle member".to_owned()
    } else {
        found.join(", ")
    }
}

/// Build one bundle read failure for the bundle.
fn unknown(bundle: &Path, message: String) -> RuntimeError {
    RuntimeError::BundleRead {
        path: bundle.to_path_buf(),
        fault: AccessFault::Unknown { message },
    }
}

/// Map one codec failure into its bundle error.
fn from_codec(bundle: &Path, error: CodecError) -> RuntimeError {
    match error {
        CodecError::Version { got } => RuntimeError::BundleVersion {
            path: bundle.to_path_buf(),
            got,
        },
        CodecError::Corrupt { message } => unknown(bundle, message),
    }
}

/// Map one blob failure into its bundle error.
fn from_blob(bundle: &Path, manifest: &Manifest, content: &Sha, error: BlobError) -> RuntimeError {
    match error {
        BlobError::Corrupt { .. } => RuntimeError::BundleCorrupt {
            path: bundle.to_path_buf(),
            holders: holders(manifest, content),
        },
        BlobError::Read {
            sha,
            fault: AccessFault::Missing,
        } => RuntimeError::BundleMissing { sha },
        BlobError::Read { fault, .. } => RuntimeError::BundleRead {
            path: bundle.to_path_buf(),
            fault,
        },
        BlobError::Write { fault, .. } => RuntimeError::BundleWrite {
            path: bundle.to_path_buf(),
            fault,
        },
        BlobError::WriteUnknown { message, .. } => RuntimeError::BundleWriteUnknown {
            path: bundle.to_path_buf(),
            message,
        },
        other => unknown(bundle, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use confit_driver as driver;
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

    fn build_bundle(dest: &Path, entries: &[(&str, Vec<u8>)]) {
        use confit_driver::archive::{Algorithm, Builder, Level};

        let build = |path: &Path| {
            let sink = std::fs::File::create(path)?;
            let mut builder = Builder::create(sink, Algorithm::TarGz, Level::default());
            for (name, bytes) in entries {
                let mut reader = std::io::Cursor::new(bytes.as_slice());
                builder.member(name, 0o644, bytes.len() as u64, &mut reader)?;
            }
            builder.finish()?;
            Ok(())
        };
        if let Err(error) = driver::atomic_write(dest, build) {
            panic!("fixture bundle builds: {error}");
        }
    }

    fn entry_names(path: &Path) -> Vec<String> {
        let file = std::fs::File::open(path).unwrap();
        driver::archive::names(file).unwrap()
    }

    #[test]
    fn round_trip_writes_manifest_first_with_sorted_blobs() {
        let dir = tempfile::tempdir().unwrap();
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
        assert_eq!(names[0], BUNDLE_MANIFEST, "the manifest comes first");
        let blobs: Vec<String> = names[1..].to_vec();
        assert_eq!(blobs.len(), 2, "every blob is included");
        let mut sorted = blobs.clone();
        sorted.sort();
        assert_eq!(blobs, sorted, "the blob entries stay sorted");
        let mut expected: Vec<String> = manifest
            .refs()
            .iter()
            .map(|blob| [BUNDLE_BLOBS_PREFIX, &blob.hex()].concat())
            .collect();
        expected.sort();
        expected.dedup();
        assert_eq!(blobs, expected, "blob entries carry content names");
        for name in &blobs {
            assert!(
                name.starts_with(BUNDLE_BLOBS_PREFIX),
                "the blob entry stays under the prefix: {name}"
            );
        }
        match store.read(written.canonical()) {
            Ok(found) => {
                assert_eq!(found, manifest, "manifest round-trips");
            }
            Err(error) => panic!("manifest load: {error}"),
        }
    }

    #[test]
    fn read_rejects_foreign_bundles() {
        let dir = tempfile::tempdir().unwrap();
        let (store, _) = opaque_manifest(dir.path(), &[]);
        let stale = serde_json::json!({"version": 0u32, "documents": [], "hashes": {}});
        let raw = serde_json::to_vec(&stale).unwrap();
        let stale_path = dir.path().join("stale.cb");
        build_bundle(&stale_path, &[(BUNDLE_MANIFEST, raw)]);
        assert!(store.read(&stale_path).is_err(), "stale version fails");
    }

    #[test]
    fn read_refuses_missing_blob() {
        use confit_store::Stores;

        let dir = tempfile::tempdir().unwrap();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(roots, sender);
        let store = Bundler::new(&stores);
        let project = dir.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("wanted.bin"), b"wanted bytes").unwrap();
        let source = match stores.resources().resource("wanted.bin") {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        };
        let handle = match stores.blobs().put(&source) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores: {error}"),
        };
        let manifest = match Manifest::build(
            &stores,
            vec![Document::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                Data::Opaque(OpaqueDocument {
                    handle: Payload::Resolved(handle.sha256().clone()),
                    mode: 0o644,
                    unmanaged: false,
                }),
            )],
            Vec::new(),
        ) {
            Ok(manifest) => manifest,
            Err(error) => panic!("manifest builds: {error}"),
        };
        let raw = serde_json::to_vec(&manifest).unwrap();
        let path = dir.path().join("thin.cb");
        build_bundle(&path, &[(BUNDLE_MANIFEST, raw)]);
        assert!(store.read(&path).is_err(), "thin manifest fails");
    }

    #[test]
    fn read_names_holding_document_for_corrupt_blob() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"alpha"]);
        let refs = manifest.refs();
        let raw = serde_json::to_vec(&manifest).unwrap();
        let member = [BUNDLE_BLOBS_PREFIX, &refs[0].hex()].concat();
        let path = dir.path().join("corrupt.cb");
        build_bundle(
            &path,
            &[
                (BUNDLE_MANIFEST, raw),
                (member.as_str(), b"garbage".to_vec()),
            ],
        );
        assert!(store.read(&path).is_err(), "corrupt blob fails");
    }

    #[test]
    fn read_rejects_duplicate_and_unexpected_and_bad_ids() {
        let dir = tempfile::tempdir().unwrap();
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
        assert!(store.read(&dup_path).is_err(), "duplicate manifest fails");

        let bad_path = dir.path().join("bad-id.cb");
        build_bundle(
            &bad_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                ("blobs/short", b"x".to_vec()),
            ],
        );
        assert!(store.read(&bad_path).is_err(), "bad id fails");

        let odd_path = dir.path().join("odd.cb");
        build_bundle(
            &odd_path,
            &[
                (BUNDLE_MANIFEST, manifest.clone()),
                ("other.txt", b"x".to_vec()),
            ],
        );
        assert!(store.read(&odd_path).is_err(), "unexpected entry fails");
    }

    #[test]
    fn write_builds_gzipped_envelope() {
        let dir = tempfile::tempdir().unwrap();
        let (store, manifest) = opaque_manifest(dir.path(), &[b"legacy bytes"]);
        let plan = dir.path().join("plan");
        let written = match store.write(&manifest, &plan) {
            Ok(written) => written,
            Err(error) => panic!("manifest writes: {error}"),
        };
        let raw = std::fs::read(written.canonical()).unwrap();
        assert!(!raw.is_empty(), "gzipped holds bytes");
        assert_eq!(
            &raw[0..2],
            &[0x1f, 0x8b],
            "gzipped envelope carries gzip magic"
        );
    }
}
