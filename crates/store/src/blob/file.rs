//! File
//!
//! File-backed blob pool.

use std::collections::BTreeSet;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use confit_core::error::{Error, Result};
use confit_core::handles::{BlobHandle, TrustedHandle};
use confit_core::ids::sha256_hex;
use confit_core::plan::BUNDLE_VERSION;
use confit_core::store::manifest::Manifest;
use sha2::Digest;

use super::BlobStore;
use crate::StoreRoots;

/// Pool folder name under the config base.
const BLOBS_DIR: &str = "blobs";

/// Pool gzip level.
const BLOB_GZIP_LEVEL: u32 = 6;

/// Blob hash length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;

/// Staging suffix for atomic pool writes.
const STAGING_SUFFIX: &str = ".part";

/// Staging counter for pooled source writes.
static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

/// Copy chunk size for trusted source streaming.
const SOURCE_CHUNK: usize = 8192;

/// Gzip footer size holding the raw byte count.
const GZIP_ISIZE_LEN: u64 = 4;

/// File-backed blob pool.
///
/// Pool files ride `{config}/blobs/{stored}` as gzip bytes.
#[derive(Debug, Clone)]
pub struct FileBlobStore {
    config_base: PathBuf,
    pool: PathBuf,
}

/// Verifying blob byte stream.
struct VerifiedBlobReader {
    decoder: flate2::read::GzDecoder<std::fs::File>,
    sha: String,
    hasher: sha2::Sha256,
    done: bool,
}

/// Staging file hashing encoded pool bytes mid-stream.
struct StoredWriter {
    file: std::fs::File,
    hasher: sha2::Sha256,
}

impl FileBlobStore {
    /// Builds a file-backed blob pool under the config base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            config_base: roots.config_base.clone(),
            pool: roots.config_base.join(BLOBS_DIR),
        }
    }
}

impl BlobStore for FileBlobStore {
    fn open(&self, handle: &BlobHandle) -> Result<Box<dyn std::io::Read>> {
        let sha = handle.sha();
        if !is_pool_id(handle.stored()) {
            return Err(Error::Plan(format!("missing blob '{sha}'")));
        }
        let path = self.pool.join(handle.stored());
        let file = std::fs::File::open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::Plan(format!("missing blob '{sha}'"))
            } else {
                Error::Plan(format!("read blob '{sha}': {error}"))
            }
        })?;
        Ok(Box::new(VerifiedBlobReader {
            decoder: flate2::read::GzDecoder::new(file),
            sha: sha.to_string(),
            hasher: sha2::Sha256::new(),
            done: false,
        }) as Box<dyn std::io::Read>)
    }

    fn put(&self, bytes: &[u8]) -> Result<BlobHandle> {
        let gzipped = gzip_bytes(bytes)?;
        let handle = BlobHandle::new(sha256_hex(bytes), sha256_hex(&gzipped))?;
        let dest = self.pool.join(handle.stored());
        if dest.exists() {
            return Ok(handle);
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        }
        let staging = self
            .pool
            .join(format!("{}{STAGING_SUFFIX}", handle.stored()));
        std::fs::write(&staging, &gzipped)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        std::fs::rename(&staging, &dest)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        Ok(handle)
    }

    fn put_source(&self, source: &dyn TrustedHandle) -> Result<BlobHandle> {
        let path = source.canonical();
        std::fs::create_dir_all(&self.pool).map_err(|error| {
            Error::Plan(format!("cannot write '{}': {error}", self.pool.display()))
        })?;
        let staging = staging_path(&self.pool);
        let mut file = std::fs::File::open(path)
            .map_err(|error| Error::Plan(format!("read source '{}': {error}", path.display())))?;
        let staged = std::fs::File::create(&staging).map_err(|error| {
            Error::Plan(format!("cannot write '{}': {error}", staging.display()))
        })?;
        let mut encoder = flate2::write::GzEncoder::new(
            StoredWriter::new(staged),
            flate2::Compression::new(BLOB_GZIP_LEVEL),
        );
        let mut chunk = [0u8; SOURCE_CHUNK];
        loop {
            let read = file.read(&mut chunk).map_err(|error| {
                Error::Plan(format!("read source '{}': {error}", path.display()))
            })?;
            if read == 0 {
                break;
            }
            encoder
                .write_all(&chunk[..read])
                .map_err(|error| Error::Plan(format!("compress blob: {error}")))?;
        }
        let writer = encoder
            .finish()
            .map_err(|error| Error::Plan(format!("compress blob: {error}")))?;
        let handle = BlobHandle::new(source.sha(), writer.digest())?;
        let dest = self.pool.join(handle.stored());
        if dest.exists() {
            std::fs::remove_file(&staging).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        } else {
            std::fs::rename(&staging, &dest).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        }
        Ok(handle)
    }

    fn has(&self, handle: &BlobHandle) -> bool {
        if !is_pool_id(handle.stored()) {
            return false;
        }
        self.pool.join(handle.stored()).exists()
    }

    fn len(&self, handle: &BlobHandle) -> Result<u64> {
        let sha = handle.sha();
        if !is_pool_id(handle.stored()) {
            return Err(Error::Plan(format!("missing blob '{sha}'")));
        }
        pooled_len(&self.pool.join(handle.stored()), sha)
    }

    fn prune(&self) -> Result<usize> {
        let mut keep: BTreeSet<String> = BTreeSet::new();
        collect_manifest_refs(&self.config_base.join("state.json"), &mut keep);
        collect_dir_refs(&self.config_base.join("previous"), &mut keep)?;
        collect_dir_refs(&self.config_base.join("plans"), &mut keep)?;
        let entries = match std::fs::read_dir(&self.pool) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(Error::from(error)),
        };
        let mut removed = 0;
        for entry in entries {
            let entry = entry.map_err(Error::from)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if keep.contains(&name) {
                continue;
            }
            std::fs::remove_file(entry.path()).map_err(Error::from)?;
            removed += 1;
        }
        Ok(removed)
    }
}

/// Compresses raw blob bytes for pool storage.
///
/// Output bytes ride gzip at the pool level.
///
/// # Errors
///
/// Encoder failures surface as plan errors.
fn gzip_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(BLOB_GZIP_LEVEL));
    encoder
        .write_all(bytes)
        .map_err(|error| Error::Plan(format!("compress blob: {error}")))?;
    encoder
        .finish()
        .map_err(|error| Error::Plan(format!("compress blob: {error}")))
}

impl StoredWriter {
    /// Wraps one staging file with an encoded-bytes hash.
    fn new(file: std::fs::File) -> Self {
        Self {
            file,
            hasher: sha2::Sha256::new(),
        }
    }

    /// Reads the encoded-bytes hash.
    fn digest(self) -> String {
        hex_digest(self.hasher)
    }
}

impl Write for StoredWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let wrote = self.file.write(buf)?;
        self.hasher.update(&buf[..wrote]);
        Ok(wrote)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

/// Scratch staging path for one pooled source write.
///
/// Names carry process plus sequence, so concurrent writes
/// never share a file.
fn staging_path(pool: &Path) -> PathBuf {
    let seq = STAGING_SEQ.fetch_add(1, Ordering::Relaxed);
    pool.join(format!(".put-{}-{seq}{STAGING_SUFFIX}", std::process::id()))
}

/// Renders one SHA-256 hash as lowercase hex.
fn hex_digest(hash: sha2::Sha256) -> String {
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl std::io::Read for VerifiedBlobReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        match self.decoder.read(buf) {
            Ok(0) => {
                if self.done {
                    return Ok(0);
                }
                self.done = true;
                let actual = hex_digest(std::mem::replace(&mut self.hasher, sha2::Sha256::new()));
                if actual != self.sha {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("blob '{}' fails verification", self.sha),
                    ));
                }
                Ok(0)
            }
            Ok(read) => {
                self.hasher.update(&buf[..read]);
                Ok(read)
            }
            Err(error) => Err(std::io::Error::other(format!(
                "read blob '{}': {error}",
                self.sha
            ))),
        }
    }
}

/// Raw byte count for one pool file.
///
/// # Errors
///
/// Missing and unreadable pool files fail as plan errors
/// naming the hash.
fn pooled_len(path: &Path, sha: &str) -> Result<u64> {
    let mut file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Plan(format!("missing blob '{sha}'"))
        } else {
            Error::Plan(format!("read blob '{sha}': {error}"))
        }
    })?;
    let size = file
        .metadata()
        .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?
        .len();
    if size < GZIP_ISIZE_LEN {
        return Err(Error::Plan(format!("read blob '{sha}': short pool file")));
    }
    file.seek(std::io::SeekFrom::End(-(GZIP_ISIZE_LEN as i64)))
        .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?;
    let mut footer = [0u8; GZIP_ISIZE_LEN as usize];
    file.read_exact(&mut footer)
        .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?;
    Ok(u32::from_le_bytes(footer) as u64)
}

/// Content hash for one source path.
///
/// # Errors
///
/// Unreadable sources fail as plan errors naming the path.
/// Pool id shape at path join.
///
/// Defense only; handles carry validity.
fn is_pool_id(sha: &str) -> bool {
    sha.len() == BLOB_ID_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Collects blob refs from every manifest file in one folder.
///
/// Unreadable and unparsable files skip quietly.
///
/// # Errors
///
/// Listing failures surface as plan or io errors.
fn collect_dir_refs(dir: &Path, keep: &mut BTreeSet<String>) -> Result<()> {
    let mut files = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(Error::from)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::from(error)),
    };
    files.sort_by_key(|entry| entry.path());
    for file in files {
        collect_manifest_refs(&file.path(), keep);
    }
    Ok(())
}

/// Collects blob refs from one manifest file without hydrating.
///
/// Missing, unparsable, and stale files add no refs.
fn collect_manifest_refs(path: &Path, keep: &mut BTreeSet<String>) {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let stored: Manifest = match serde_json::from_slice(&bytes) {
        Ok(stored) => stored,
        Err(_) => return,
    };
    if stored.version != BUNDLE_VERSION {
        return;
    }
    for document in &stored.documents {
        keep.extend(
            document
                .data
                .blob_handles()
                .into_iter()
                .map(|handle| handle.stored().to_string()),
        );
    }
}
