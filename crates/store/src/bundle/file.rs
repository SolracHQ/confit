//! File
//!
//! File-backed bundle archives.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use confit_core::error::{Error, Result};
use confit_core::handles::{BlobHandle, Sha};
use confit_core::plan::{BUNDLE_VERSION, Bundle};
use confit_core::progress::ProgressSender;
use confit_core::store::manifest::Manifest;
use sha2::Digest as _;

use super::BundleStore;
use crate::StoreRoots;

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

/// File bundle archive reads and writes.
///
/// Archives hold the manifest first with blob entries after.
#[derive(Debug, Clone)]
pub struct FileBundleStore {
    pool: PathBuf,
}

impl FileBundleStore {
    /// Builds a file bundle store under the config base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            pool: roots.config_base.join(BLOBS_DIR),
        }
    }
}

impl BundleStore for FileBundleStore {
    fn write(
        &self,
        bundle: &Bundle,
        dest: &Path,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf> {
        let _ = progress;
        let dest = super::ensure_bundle_extension(dest);
        let manifest = serde_json::to_vec_pretty(&bundle.manifest)
            .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
        let encoder =
            flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(BUNDLE_GZIP_LEVEL));
        let mut builder = tar::Builder::new(encoder);
        append_bundle_entry(&mut builder, BUNDLE_MANIFEST, &manifest, &dest)?;
        for handle in bundle.blobs.values() {
            append_pool_entry(&mut builder, handle.stored(), &self.pool, &dest)?;
        }
        let encoder = builder
            .into_inner()
            .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
        let archive = encoder
            .finish()
            .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        }
        let mut staging = dest.as_os_str().to_owned();
        staging.push(STAGING_SUFFIX);
        let staging = PathBuf::from(staging);
        std::fs::write(&staging, &archive)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        std::fs::rename(&staging, &dest)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        Ok(dest)
    }

    fn read(&self, path: &Path) -> Result<Bundle> {
        let file = std::fs::File::open(path)
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
                let _ = std::fs::remove_file(staging);
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
                let _ = std::fs::remove_file(staging);
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
    builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
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
    builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
    stored: &Sha,
    pool: &Path,
    dest: &Path,
) -> Result<()> {
    let pooled = pool.join(stored.hex());
    let source = match std::fs::File::open(&pooled) {
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
    let len = source
        .metadata()
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
    std::fs::create_dir_all(pool)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", pool.display())))?;
    let staging = pool.join(format!("{stored}{STAGING_SUFFIX}"));
    let mut staged = std::fs::File::create(&staging)
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
        let _ = std::fs::remove_file(&staging);
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
    let file = std::fs::File::open(staging)
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
        let _ = std::fs::remove_file(staging);
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
    if dest.exists() {
        std::fs::remove_file(staging)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
    } else {
        std::fs::rename(staging, &dest)
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
