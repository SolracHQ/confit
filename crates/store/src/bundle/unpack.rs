//! Unpack
//!
//! Bundle reads over archive extraction and blob receives.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read as _;
use std::path::Path;

use confit_model::document::BlobRef;
use confit_model::error::{Error, Result};
use confit_model::manifest::Manifest;
use confit_model::sha::Sha;

use super::{BUNDLE_BLOBS_PREFIX, BUNDLE_MANIFEST, BUNDLE_VERSION, Bundle, BundleStore};
use crate::handles::ArchiveHandle;

/// Blob hash length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;

impl BundleStore {
    /// Reads one portable bundle into a live bundle.
    ///
    /// # Errors
    ///
    /// Unreadable files and bad payloads fail as plan errors.
    pub fn read(&self, path: &Path) -> Result<Bundle> {
        let handle = self
            .archives
            .seal(path)
            .map_err(|error| read_failure(path, error))?;
        let names = self
            .archives
            .members(&handle)
            .map_err(|error| read_failure(path, error))?;
        let (has_manifest, wanted) = check_names(&names, path)?;
        if !has_manifest {
            return Err(Error::Plan(format!(
                "read bundle '{}': missing manifest",
                path.display()
            )));
        }
        let manifest = self.read_manifest(&handle, path)?;
        if manifest.version != BUNDLE_VERSION {
            return Err(Error::Plan(format!(
                "bundle version {} reads unsupported, want {BUNDLE_VERSION}",
                manifest.version
            )));
        }
        let refs = manifest_refs(&manifest);
        for blob in &refs {
            if !wanted
                .iter()
                .any(|name| name == blob.stored().hex().as_str())
            {
                return Err(Error::Plan(format!(
                    "read bundle '{}': missing blob '{}'",
                    path.display(),
                    blob.sha()
                )));
            }
        }
        for stored in &wanted {
            self.receive_blob(&handle, stored, path)?;
        }
        let mut blobs: BTreeMap<String, BlobRef> = BTreeMap::new();
        for blob in &refs {
            blobs
                .entry(blob.sha().hex())
                .or_insert_with(|| blob.clone());
        }
        Ok(Bundle { manifest, blobs })
    }

    /// Reads and parses the bundle manifest member.
    ///
    /// # Errors
    ///
    /// Unreadable members and bad payloads fail as plan errors
    /// naming the bundle.
    fn read_manifest(&self, handle: &ArchiveHandle, bundle: &Path) -> Result<Manifest> {
        let member = self
            .archives
            .extract_member(handle, BUNDLE_MANIFEST)
            .map_err(|error| read_failure(bundle, error))?;
        let mut reader = self
            .archives
            .open_decompressed(&member)
            .map_err(|error| read_failure(bundle, error))?;
        let mut raw = Vec::new();
        reader
            .read_to_end(&mut raw)
            .map_err(|error| read_failure(bundle, error))?;
        serde_json::from_slice(&raw).map_err(|error| read_failure(bundle, error))
    }

    /// Receives one blob member into the cache blind.
    ///
    /// # Errors
    ///
    /// Unreadable members and bad ids fail as plan errors naming
    /// the bundle. Cache write failures surface as plan errors
    /// naming the destination.
    fn receive_blob(&self, handle: &ArchiveHandle, stored: &str, bundle: &Path) -> Result<()> {
        let name = format!("{BUNDLE_BLOBS_PREFIX}{stored}");
        let member = self
            .archives
            .extract_member(handle, &name)
            .map_err(|error| read_failure(bundle, error))?;
        let stored_sha = Sha::new(stored).map_err(|_| {
            Error::Plan(format!(
                "read bundle '{}': bad blob entry '{stored}'",
                bundle.display()
            ))
        })?;
        self.blobs.receive(&stored_sha, &member)?;
        Ok(())
    }
}

/// Validates bundle member names with verbatim policy.
///
/// # Errors
///
/// Duplicate, unexpected, and malformed entries fail as plan
/// errors naming the bundle.
fn check_names(names: &[String], bundle: &Path) -> Result<(bool, Vec<String>)> {
    let mut has_manifest = false;
    let mut wanted: Vec<String> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for name in names {
        if name == BUNDLE_MANIFEST {
            if has_manifest {
                return Err(Error::Plan(format!(
                    "read bundle '{}': duplicate manifest",
                    bundle.display()
                )));
            }
            has_manifest = true;
        } else if let Some(stored) = name.strip_prefix(BUNDLE_BLOBS_PREFIX) {
            check_blob_id(stored)?;
            if !seen.insert(stored.to_string()) {
                return Err(Error::Plan(format!(
                    "read bundle '{}': duplicate blob '{stored}'",
                    bundle.display()
                )));
            }
            wanted.push(stored.to_string());
        } else {
            return Err(Error::Plan(format!(
                "read bundle '{}': unexpected entry '{name}'",
                bundle.display()
            )));
        }
    }
    Ok((has_manifest, wanted))
}

/// Cloned blob refs for one manifest in document order.
fn manifest_refs(manifest: &Manifest) -> Vec<BlobRef> {
    let mut refs = Vec::new();
    for document in &manifest.documents {
        refs.extend(document.data.blob_refs().into_iter().cloned());
    }
    refs
}

/// Builds one bundle read error naming the bundle.
fn read_failure(bundle: &Path, error: impl std::fmt::Display) -> Error {
    Error::Plan(format!("read bundle '{}': {error}", bundle.display()))
}

/// Checks one blob reference holds 64 hex chars.
///
/// # Errors
///
/// Malformed references fail as plan errors naming the value.
fn check_blob_id(sha: &str) -> Result<()> {
    if sha.len() == BLOB_ID_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::Plan(format!(
            "bad blob ref '{sha}': want {BLOB_ID_LEN} hex chars"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{ManifestData, ManifestDocument};
    use confit_model::routes::{Route, RouteBase};

    use crate::StoreRoots;
    use crate::archive::ArchiveStore;
    use crate::blob::{BlobSource, BlobStore};
    use confit_driver as driver;
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
    fn round_trip_writes_manifest_first_with_sorted_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, bundle) = opaque_bundle(dir.path(), &[b"alpha", b"beta"]);
        let dest = match store.write(&bundle, &dir.path().join("plan"), None) {
            Ok(dest) => dest,
            Err(error) => panic!("bundle writes: {error}"),
        };
        assert_eq!(dest, dir.path().join("plan.cb"), "bare output gains suffix");
        let names = entry_names(&dest);
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
        match store.read(&dest) {
            Ok(found) => {
                assert_eq!(found.manifest, bundle.manifest, "manifest round-trips");
                assert_eq!(found.blobs, bundle.blobs, "handles round-trip");
            }
            Err(error) => panic!("bundle reads: {error}"),
        }
    }

    #[test]
    fn read_refuses_stale_version() {
        use flate2::write::GzEncoder;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, _) = opaque_bundle(dir.path(), &[]);
        let stale = serde_json::json!({"version": 0u32, "documents": []});
        let raw = serde_json::to_vec(&stale).unwrap();
        let encoder = GzEncoder::new(Vec::new(), flate2::Compression::new(0));
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(raw.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, BUNDLE_MANIFEST, raw.as_slice())
            .unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();
        let path = dir.path().join("stale.cb");
        driver::create_dir_all(path.parent().unwrap()).unwrap();
        driver::write(&path, &archive).unwrap();
        match store.read(&path) {
            Ok(_) => panic!("stale bundle passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("unsupported"), "stale reports itself: {text}");
                assert!(
                    text.contains(&BUNDLE_VERSION.to_string()),
                    "stale names the want: {text}"
                );
            }
        }
    }

    #[test]
    fn read_refuses_missing_blob() {
        use flate2::write::GzEncoder;
        use std::sync::Arc;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let archives = Arc::new(ArchiveStore::new(&roots));
        let blobs = Arc::new(BlobStore::new(&roots));
        let store = BundleStore::new(archives, blobs.clone());
        let handle = match blobs.put(BlobSource::Bytes(b"wanted bytes")) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores: {error}"),
        };
        let mut bundle = match Bundle::build(
            vec![ManifestDocument::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                ManifestData::Opaque {
                    blob: handle.to_ref(),
                    size: 12,
                    mode: None,
                    unmanaged: false,
                },
            )],
            Vec::new(),
        ) {
            Ok(bundle) => bundle,
            Err(error) => panic!("bundle builds: {error}"),
        };
        bundle.blobs.insert(handle.sha().hex(), handle.to_ref());
        let raw = serde_json::to_vec(&bundle.manifest).unwrap();
        let encoder = GzEncoder::new(Vec::new(), flate2::Compression::new(0));
        let mut builder = tar::Builder::new(encoder);
        let mut header = tar::Header::new_gnu();
        header.set_size(raw.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, BUNDLE_MANIFEST, raw.as_slice())
            .unwrap();
        let archive = builder.into_inner().unwrap().finish().unwrap();
        let path = dir.path().join("thin.cb");
        driver::create_dir_all(path.parent().unwrap()).unwrap();
        driver::write(&path, &archive).unwrap();
        match store.read(&path) {
            Ok(_) => panic!("thin bundle passes"),
            Err(error) => assert!(
                error.to_string().contains(&handle.sha().hex()),
                "missing blob names the hash: {error}"
            ),
        }
    }

    #[test]
    fn read_rejects_duplicate_and_unexpected_and_bad_ids() {
        use flate2::write::GzEncoder;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (store, _) = opaque_bundle(dir.path(), &[]);

        let build_bundle = |entries: Vec<(&str, Vec<u8>)>| {
            let encoder = GzEncoder::new(Vec::new(), flate2::Compression::new(0));
            let mut builder = tar::Builder::new(encoder);
            for (name, bytes) in entries {
                let mut header = tar::Header::new_gnu();
                header.set_size(bytes.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder
                    .append_data(&mut header, name, bytes.as_slice())
                    .unwrap();
            }
            builder.into_inner().unwrap().finish().unwrap()
        };

        let manifest = serde_json::to_vec(&Bundle::empty().manifest).unwrap();
        let dup_manifest = build_bundle(vec![
            (BUNDLE_MANIFEST, manifest.clone()),
            (BUNDLE_MANIFEST, manifest.clone()),
        ]);
        let dup_path = dir.path().join("dup-manifest.cb");
        driver::create_dir_all(dup_path.parent().unwrap()).unwrap();
        driver::write(&dup_path, &dup_manifest).unwrap();
        match store.read(&dup_path) {
            Ok(_) => panic!("duplicate manifest passes"),
            Err(error) => assert!(
                error.to_string().contains("duplicate manifest"),
                "duplicate reports itself: {error}"
            ),
        }

        let bad_id = build_bundle(vec![
            (BUNDLE_MANIFEST, manifest.clone()),
            ("blobs/short", b"x".to_vec()),
        ]);
        let bad_path = dir.path().join("bad-id.cb");
        driver::create_dir_all(bad_path.parent().unwrap()).unwrap();
        driver::write(&bad_path, &bad_id).unwrap();
        match store.read(&bad_path) {
            Ok(_) => panic!("bad id passes"),
            Err(error) => assert!(
                error.to_string().contains("bad blob"),
                "bad id reports itself: {error}"
            ),
        }

        let unexpected = build_bundle(vec![
            (BUNDLE_MANIFEST, manifest.clone()),
            ("other.txt", b"x".to_vec()),
        ]);
        let odd_path = dir.path().join("odd.cb");
        driver::create_dir_all(odd_path.parent().unwrap()).unwrap();
        driver::write(&odd_path, &unexpected).unwrap();
        match store.read(&odd_path) {
            Ok(_) => panic!("unexpected entry passes"),
            Err(error) => assert!(
                error.to_string().contains("unexpected entry"),
                "unexpected reports itself: {error}"
            ),
        }
    }
}
