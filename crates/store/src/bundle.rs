//! Bundle
//!
//! Portable bundle archives holding manifests and blobs.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use confit_model::document::ManifestDocument;
use confit_model::error::{Error, Result};
use confit_model::handles::{BlobHandle, Sha};
use confit_model::hook::Hook;
use confit_model::manifest::Manifest;
use confit_model::plan::{DocumentStatus, Summary};
use confit_model::progress::ProgressSender;
use sha2::Digest as _;

use crate::StoreRoots;
use confit_driver as driver;

/// Bundle file extension imposed on explicit outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Gzip level for the outer bundle tar.
const BUNDLE_GZIP_LEVEL: u32 = 0;

/// Bundle manifest file name inside the archive.
const BUNDLE_MANIFEST: &str = "manifest.json";

/// Bundle blob folder prefix inside the archive.
const BUNDLE_BLOBS_PREFIX: &str = "blobs/";

/// Pool folder name under the config base.
const BLOBS_DIR: &str = "blobs";

/// Stream chunk size for verifying pooled bytes.
const VERIFY_CHUNK: usize = 8192;

/// Blob hash length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;

/// Staging suffix for atomic bundle writes.
const STAGING_SUFFIX: &str = ".part";

/// Ensures one bundle destination carries the bundle extension.
///
/// Bare paths gain the suffix, so creators always emit
/// bundles. Slot outputs never pass here and keep their
/// own names.
pub fn ensure_bundle_extension(dest: &Path) -> PathBuf {
    if dest
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(BUNDLE_EXTENSION))
    {
        dest.to_path_buf()
    } else {
        let mut name = dest.as_os_str().to_owned();
        name.push(".cb");
        PathBuf::from(name)
    }
}

/// Bundle format version written by every bundle build.
///
pub const BUNDLE_VERSION: u32 = 7;

/// Versioned desired state written by bundle builds.
///
/// The manifest holds version, documents, and
/// hooks as the only document language. The blob map holds
/// blob handles under content hashes beside it.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Holds the portable manifest as the only document language.
    pub manifest: Manifest,
    /// Holds blob handles under SHA-256 hex hashes.
    pub blobs: BTreeMap<String, BlobHandle>,
}

impl Bundle {
    /// Builds an empty manifest with the current version.
    ///
    /// # Returns
    ///
    /// The bundle holding version and empty documents.
    ///
    pub fn empty() -> Self {
        Self {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents: Vec::new(),
                hooks: Vec::new(),
            },
            blobs: BTreeMap::new(),
        }
    }

    /// Builds the desired state bundle from documents.
    ///
    /// Fills data hashes, then sorts documents by destination.
    /// The caller holds one document per destination.
    /// Counts generate through `summary` against a previous manifest.
    ///
    /// # Arguments
    ///
    /// * `documents` - desired documents in pipeline order, unique per destination.
    /// * `hooks` - desired hooks in declaration order, merged downstream.
    ///
    /// # Returns
    ///
    /// The built bundle.
    ///
    /// # Errors
    ///
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{ManifestData, ManifestDocument};
    /// use confit_model::handles::{Route, RouteBase};
    /// use confit_store::bundle::Bundle;
    ///
    /// let document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// let outcome = Bundle::build(vec![document], Vec::new());
    /// let previous = Bundle::empty();
    /// assert!(matches!(outcome, Ok(bundle) if bundle.summary(&previous).create == 1));
    /// ```
    pub fn build(mut documents: Vec<ManifestDocument>, hooks: Vec<Hook>) -> Result<Self> {
        for document in &mut documents {
            document.fill_hash()?;
        }
        documents.sort_by_key(|document| document.destination.display());
        Ok(Self {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents,
                hooks,
            },
            blobs: BTreeMap::new(),
        })
    }

    /// Counts lifecycle states against a previous manifest.
    ///
    /// Opaque kind changes count as updates, other kind changes
    /// count as create and delete.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous manifest with filled hashes.
    ///
    /// # Returns
    ///
    /// Create, update, and delete counts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{ManifestData, ManifestDocument};
    /// use confit_model::handles::{Route, RouteBase};
    /// use confit_store::bundle::Bundle;
    ///
    /// let mut previous = Bundle::empty();
    /// previous.manifest.documents = vec![ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// )];
    /// let bundle = Bundle::build(
    ///     vec![ManifestDocument::new(
    ///         Route::new(RouteBase::Home, "note").unwrap(),
    ///         ManifestData::Text { content: "changed".into(), mode: None, unmanaged: false},
    ///     )],
    ///     Vec::new(),
    /// );
    /// assert!(matches!(bundle, Ok(bundle) if bundle.summary(&previous).update == 1));
    /// ```
    pub fn summary(&self, previous: &Bundle) -> Summary {
        let mut summary = Summary {
            create: 0,
            update: 0,
            delete: 0,
        };
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for document in &self.manifest.documents {
            seen.insert(document.key());
            match document.status(&previous.manifest) {
                DocumentStatus::Create => summary.create += 1,
                DocumentStatus::Update => summary.update += 1,
                DocumentStatus::Unchanged => {}
            }
        }
        for recorded in &previous.manifest.documents {
            if !seen.contains(&recorded.key()) && !recorded.superseded_by(&self.manifest.documents)
            {
                summary.delete += 1;
            }
        }
        summary
    }
}

/// Portable bundle archive reads and writes.
///
/// Archives hold the manifest first with blob entries after.
#[derive(Debug, Clone)]
pub struct BundleStore {
    pool: PathBuf,
}

impl BundleStore {
    /// Builds a file bundle store under the config base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            pool: roots.config_base.join(BLOBS_DIR),
        }
    }

    /// Writes one portable bundle holding manifest and blobs.
    ///
    /// Bare destinations gain the bundle extension; the
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
        append_bundle_entry(&mut builder, BUNDLE_MANIFEST, &manifest, &dest)?;
        for handle in bundle.blobs.values() {
            append_pool_entry(&mut builder, handle.stored(), &self.pool, &dest)?;
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

    /// Reads one portable bundle into a live bundle.
    ///
    /// # Errors
    ///
    /// Unreadable files and bad payloads fail as plan errors.
    pub fn read(&self, path: &Path) -> Result<Bundle> {
        let file = driver::open_read(path)
            .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
        let decoder = flate2::read::GzDecoder::new(file);
        let mut archive = tar::Archive::new(decoder);
        let mut manifest: Option<Manifest> = None;
        let mut staged: Vec<(String, PathBuf)> = Vec::new();
        let entries = archive
            .entries()
            .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
        for entry in entries {
            let mut entry = entry.map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            let entry_path = entry
                .path()
                .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?
                .into_owned();
            if entry_path == Path::new(BUNDLE_MANIFEST) {
                if manifest.is_some() {
                    return Err(Error::Plan(format!(
                        "read bundle '{}': duplicate manifest",
                        path.display()
                    )));
                }
                let mut raw = Vec::new();
                entry.read_to_end(&mut raw).map_err(|error| {
                    Error::Plan(format!("read bundle '{}': {error}", path.display()))
                })?;
                let stored: Manifest = serde_json::from_slice(&raw).map_err(|error| {
                    Error::Plan(format!("read bundle '{}': {error}", path.display()))
                })?;
                if stored.version != BUNDLE_VERSION {
                    return Err(Error::Plan(format!(
                        "bundle version {} reads unsupported, want {BUNDLE_VERSION}",
                        stored.version
                    )));
                }
                manifest = Some(stored);
            } else if let Ok(rel) = entry_path.strip_prefix(BUNDLE_BLOBS_PREFIX) {
                let stored = match rel.to_str() {
                    Some(stored) => stored.to_string(),
                    None => {
                        return Err(Error::Plan(format!(
                            "read bundle '{}': bad blob entry",
                            path.display()
                        )));
                    }
                };
                if check_blob_id(&stored).is_err() {
                    return Err(Error::Plan(format!(
                        "read bundle '{}': bad blob entry '{stored}'",
                        path.display()
                    )));
                }
                if staged.iter().any(|(known, _)| known == &stored) {
                    return Err(Error::Plan(format!(
                        "read bundle '{}': duplicate blob '{stored}'",
                        path.display()
                    )));
                }
                let staging = stage_entry_to_pool(&mut entry, &stored, &self.pool, path)?;
                staged.push((stored, staging));
            } else {
                return Err(Error::Plan(format!(
                    "read bundle '{}': unexpected entry '{}'",
                    path.display(),
                    entry_path.display()
                )));
            }
        }
        let Some(stored) = manifest else {
            for (_, staging) in &staged {
                let _ = driver::remove_file(staging);
            }
            return Err(Error::Plan(format!(
                "read bundle '{}': missing manifest",
                path.display()
            )));
        };
        let manifest_handles = collect_manifest_handles(&stored);
        let mut want_content: BTreeMap<String, String> = BTreeMap::new();
        let mut want_stored: BTreeMap<String, String> = BTreeMap::new();
        let mut handles: BTreeMap<String, BlobHandle> = BTreeMap::new();
        for handle in &manifest_handles {
            want_content.insert(handle.stored().hex(), handle.sha().hex());
            want_stored.insert(handle.sha().hex(), handle.stored().hex());
            handles
                .entry(handle.sha().hex())
                .or_insert_with(|| handle.clone());
        }
        let mut sizes: BTreeMap<String, u64> = BTreeMap::new();
        for (entry_stored, staging) in &staged {
            let expected = want_content.get(entry_stored).map(String::as_str);
            let (content, len) = verify_staged_content(staging, entry_stored, expected, path)?;
            if let Some(want) = want_stored.get(&content)
                && want.as_str() != entry_stored.as_str()
            {
                let _ = driver::remove_file(staging);
                return Err(Error::Plan(format!(
                    "read bundle '{}': bad blob entry '{entry_stored}'",
                    path.display()
                )));
            }
            if !handles.contains_key(&content) {
                let content_sha = Sha::new(content.clone()).map_err(|_| {
                    Error::Plan(format!(
                        "read bundle '{}': bad blob entry '{entry_stored}'",
                        path.display()
                    ))
                })?;
                let stored_sha = Sha::new(entry_stored.as_str()).map_err(|_| {
                    Error::Plan(format!(
                        "read bundle '{}': bad blob entry '{entry_stored}'",
                        path.display()
                    ))
                })?;
                let handle = BlobHandle::new(content_sha, stored_sha).map_err(|_| {
                    Error::Plan(format!(
                        "read bundle '{}': bad blob entry '{entry_stored}'",
                        path.display()
                    ))
                })?;
                handles.insert(content.clone(), handle);
            }
            land_staged_blob(staging, entry_stored, &self.pool)?;
            sizes.insert(content, len);
        }
        for document in &stored.documents {
            for handle in document.data.blob_handles() {
                if !sizes.contains_key(handle.sha().hex().as_str()) {
                    return Err(Error::Plan(format!(
                        "read bundle '{}': missing blob '{}'",
                        path.display(),
                        handle.sha()
                    )));
                }
            }
        }
        Ok(Bundle {
            manifest: stored,
            blobs: handles,
        })
    }
}

/// Appends one file entry to a bundle archive.
///
/// # Errors
///
/// Archive failures surface as plan errors naming the bundle.
fn append_bundle_entry(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Box<dyn std::io::Write>>>,
    name: &str,
    bytes: &[u8],
    dest: &Path,
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, name, bytes)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))
}

/// Streams one pool file into a bundle entry verbatim.
///
/// # Errors
///
/// Missing pool files and archive failures surface as plan
/// errors naming the bundle.
fn append_pool_entry(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Box<dyn std::io::Write>>>,
    stored: &Sha,
    pool: &Path,
    dest: &Path,
) -> Result<()> {
    let pooled = pool.join(stored.hex());
    let source = match driver::open_read(&pooled) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Error::Plan(format!(
                "render bundle '{}': missing blob '{stored}'",
                dest.display()
            )));
        }
        Err(error) => {
            return Err(Error::Plan(format!(
                "render bundle '{}': read blob '{stored}': {error}",
                dest.display()
            )));
        }
    };
    let len = driver::metadata(&pooled)
        .map_err(|error| {
            Error::Plan(format!(
                "render bundle '{}': read blob '{stored}': {error}",
                dest.display()
            ))
        })?
        .len();
    let mut header = tar::Header::new_gnu();
    header.set_size(len);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(
            &mut header,
            format!("{BUNDLE_BLOBS_PREFIX}{stored}"),
            source,
        )
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))
}

/// Cloned blob handles for one manifest in document order.
fn collect_manifest_handles(manifest: &Manifest) -> Vec<BlobHandle> {
    let mut handles = Vec::new();
    for document in &manifest.documents {
        handles.extend(document.data.blob_handles().into_iter().cloned());
    }
    handles
}

/// Stages one bundle entry to a pool scratch file with stored proof.
///
/// # Errors
///
/// Write and stored-hash mismatch failures surface as plan errors
/// naming the bundle plus hash. Mismatches remove staging.
fn stage_entry_to_pool(
    entry: &mut impl std::io::Read,
    stored: &str,
    pool: &Path,
    bundle: &Path,
) -> Result<PathBuf> {
    driver::create_dir_all(pool)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", pool.display())))?;
    let staging = pool.join(format!("{stored}{STAGING_SUFFIX}"));
    let mut staged = driver::create(&staging)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", staging.display())))?;
    let mut hasher = sha2::Sha256::new();
    let mut chunk = [0u8; VERIFY_CHUNK];
    loop {
        let read = entry.read(&mut chunk).map_err(|error| {
            Error::Plan(format!(
                "read bundle '{}': read blob '{stored}': {error}",
                bundle.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
        staged.write_all(&chunk[..read]).map_err(|error| {
            Error::Plan(format!("cannot write '{}': {error}", staging.display()))
        })?;
    }
    staged
        .flush()
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", staging.display())))?;
    if Sha::finish(hasher).hex().as_str() != stored {
        let _ = driver::remove_file(&staging);
        return Err(Error::Plan(format!(
            "read bundle '{}': blob '{stored}' fails verification",
            bundle.display()
        )));
    }
    Ok(staging)
}

/// Reads the verified content hash plus raw length for one staged entry.
///
/// # Errors
///
/// Decoder and content mismatch failures surface as plan
/// errors naming the bundle. Mismatches remove staging.
fn verify_staged_content(
    staging: &Path,
    stored: &str,
    expected: Option<&str>,
    bundle: &Path,
) -> Result<(String, u64)> {
    let file = driver::open_read(staging)
        .map_err(|error| Error::Plan(format!("read blob '{stored}': {error}")))?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut hasher = sha2::Sha256::new();
    let mut len = 0u64;
    let mut chunk = [0u8; VERIFY_CHUNK];
    loop {
        let read = decoder.read(&mut chunk).map_err(|error| {
            Error::Plan(format!(
                "read bundle '{}': read blob '{stored}': {error}",
                bundle.display()
            ))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&chunk[..read]);
        len += read as u64;
    }
    let content = Sha::finish(hasher).hex();
    if let Some(want) = expected
        && want != content
    {
        let _ = driver::remove_file(staging);
        return Err(Error::Plan(format!(
            "read bundle '{}': blob '{want}' fails verification",
            bundle.display()
        )));
    }
    Ok((content, len))
}

/// Moves one staged entry into the pool.
///
/// A present destination wins, so repeat reads share pool files.
///
/// # Errors
///
/// Removal and rename failures surface as plan errors
/// naming the destination.
fn land_staged_blob(staging: &Path, stored: &str, pool: &Path) -> Result<()> {
    let dest = pool.join(stored);
    if driver::exists(&dest) {
        driver::remove_file(staging)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
    } else {
        driver::rename(staging, &dest)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
    }
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
    use confit_model::handles::{Route, RouteBase};

    use crate::blob::BlobStore;
    use confit_driver::TestGuard;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            ..Default::default()
        }
    }

    fn opaque_bundle(dir: &Path, bodies: &[&[u8]]) -> (BundleStore, Bundle) {
        let roots = test_roots(dir);
        let pools = BlobStore::new(&roots);
        let mut documents = Vec::new();
        let mut bundle = Bundle::empty();
        for (index, body) in bodies.iter().enumerate() {
            let handle = match pools.put(body) {
                Ok(handle) => handle,
                Err(error) => panic!("pool stores: {error}"),
            };
            documents.push(ManifestDocument::new(
                Route::new(RouteBase::Home, format!("bin-{index}").as_str()).unwrap(),
                ManifestData::Opaque {
                    blob: handle.clone(),
                    size: body.len() as u64,
                    mode: None,
                    unmanaged: false,
                },
            ));
            bundle.blobs.insert(handle.sha().hex(), handle);
        }
        match Bundle::build(documents, Vec::new()) {
            Ok(built) => {
                bundle.manifest = built.manifest;
                (BundleStore::new(&roots), bundle)
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
    fn bare_output_gains_cb_suffix() {
        let out = ensure_bundle_extension(Path::new("plan"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("bare output gains suffix"),
        }
    }

    #[test]
    fn cb_suffix_stays_unchanged() {
        let out = ensure_bundle_extension(Path::new("plan.cb"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("cb output stays unchanged"),
        }
    }

    #[test]
    fn cb_suffix_match_reads_case_insensitive() {
        let out = ensure_bundle_extension(Path::new("plan.CB"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.CB"),
            None => panic!("uppercase cb stays untouched"),
        }
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

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let pools = BlobStore::new(&roots);
        let store = BundleStore::new(&roots);
        let handle = match pools.put(b"wanted bytes") {
            Ok(handle) => handle,
            Err(error) => panic!("pool stores: {error}"),
        };
        let mut bundle = match Bundle::build(
            vec![ManifestDocument::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                ManifestData::Opaque {
                    blob: handle.clone(),
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
        bundle.blobs.insert(handle.sha().hex(), handle.clone());
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
}
