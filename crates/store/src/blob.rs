//! Blob
//!
//! Content-addressed blob pool behind stored hashes.

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use confit_model::document::BlobRef;
use confit_model::error::{Error, Result};
use confit_model::manifest::Manifest;
use confit_model::sha::Sha;
use sha2::Digest;

use crate::StoreRoots;
use crate::bundle::BUNDLE_VERSION;
use crate::handles::{BlobHandle, TrustedHandle};
use confit_driver as driver;

/// Pool folder name under the config base.
const BLOBS_DIR: &str = "blobs";

/// Pool gzip level.
const BLOB_GZIP_LEVEL: u32 = 6;

/// Staging suffix for atomic pool writes.
const STAGING_SUFFIX: &str = ".part";

/// Staging counter for pooled source writes.
static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

/// Copy chunk size for trusted source streaming.
const SOURCE_CHUNK: usize = 8192;

/// Gzip footer size holding the raw byte count.
const GZIP_ISIZE_LEN: u64 = 4;

/// Content-addressed blob pool.
///
/// Plan cache files ride `{cache}/blobs/{stored}` as gzip bytes.
/// Named pool files ride `{config}/blobs/{stored}` as gzip bytes.
/// Spill verbs write into the cache. The pool fills
/// through persist alone at apply end.
#[derive(Debug, Clone)]
pub struct BlobStore {
    config_base: PathBuf,
    pool: PathBuf,
    cache: PathBuf,
}

/// Verifying blob byte stream.
struct VerifiedBlobReader {
    decoder: flate2::read::GzDecoder<Box<dyn std::io::Read>>,
    sha: Sha,
    hasher: sha2::Sha256,
    done: bool,
}

/// Staging file hashing encoded pool bytes mid-stream.
struct StoredWriter {
    file: Box<dyn std::io::Write>,
    hasher: sha2::Sha256,
}

/// Blob spill source shapes.
///
/// One verb spills every shape; the shape names the trust.
/// Raw bytes hash directly. Trusted files carry the sealed
/// content hash instead.
pub enum BlobSource<'a> {
    /// Raw bytes under hashing.
    Bytes(&'a [u8]),
    /// Trusted file with sealed content hash.
    Handle(&'a dyn TrustedHandle),
}

impl BlobStore {
    /// Builds a file-backed blob pool under the config base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            config_base: roots.config_base.clone(),
            pool: roots.config_base.join(BLOBS_DIR),
            cache: roots.cache_base.join(BLOBS_DIR),
        }
    }

    /// Resolves one manifest ref into its handle.
    ///
    /// # Arguments
    ///
    /// * `blob` - the manifest ref under resolving.
    ///
    /// # Returns
    ///
    /// The handle sealing both hashes.
    ///
    /// # Errors
    ///
    /// Missing blobs fail as plan errors naming the hash.
    pub fn resolve(&self, blob: &BlobRef) -> Result<BlobHandle> {
        if self.present(blob.stored()) {
            BlobHandle::new(blob.sha().clone(), blob.stored().clone())
        } else {
            Err(Error::Plan(format!("missing blob '{}'", blob.sha())))
        }
    }

    /// Opens a raw byte stream for one handle.
    ///
    /// Reads ride the stored hash from the cache with
    /// fallback to the pool; bytes verify against
    /// the content hash mid-stream.
    ///
    /// # Errors
    ///
    /// Missing blobs and verification failures fail as plan errors
    /// naming the hash.
    pub fn open(&self, handle: &BlobHandle) -> Result<Box<dyn std::io::Read>> {
        let sha = handle.sha().clone();
        let path = self.live_path(handle);
        let file = driver::open_read(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::Plan(format!("missing blob '{sha}'"))
            } else {
                Error::Plan(format!("read blob '{sha}': {error}"))
            }
        })?;
        Ok(Box::new(VerifiedBlobReader {
            decoder: flate2::read::GzDecoder::new(file),
            sha,
            hasher: sha2::Sha256::new(),
            done: false,
        }) as Box<dyn std::io::Read>)
    }

    /// Spills bytes into the cache sealing both identities.
    ///
    /// # Arguments
    ///
    /// * `source` - the bytes or trusted file under spilling.
    ///
    /// # Errors
    ///
    /// Missing sources fail as plan errors naming the path.
    /// Cache write failures surface as plan or io errors.
    pub fn put(&self, source: BlobSource<'_>) -> Result<BlobHandle> {
        let (mut reader, sha, origin): (Box<dyn Read>, Sha, Option<PathBuf>) = match source {
            BlobSource::Bytes(bytes) => (
                Box::new(std::io::Cursor::new(bytes)) as Box<dyn Read>,
                Sha::hash(bytes),
                None,
            ),
            BlobSource::Handle(handle) => {
                let path = handle.canonical();
                let file = driver::open_read(path).map_err(|error| {
                    Error::Plan(format!("read source '{}': {error}", path.display()))
                })?;
                (file, handle.sha().clone(), Some(path.to_path_buf()))
            }
        };
        driver::create_dir_all(&self.cache).map_err(|error| {
            Error::Plan(format!("cannot write '{}': {error}", self.cache.display()))
        })?;
        let staging = staging_path(&self.cache);
        let staged = driver::create(&staging).map_err(|error| {
            Error::Plan(format!("cannot write '{}': {error}", staging.display()))
        })?;
        let mut encoder = flate2::write::GzEncoder::new(
            StoredWriter::new(staged),
            flate2::Compression::new(BLOB_GZIP_LEVEL),
        );
        let mut chunk = [0u8; SOURCE_CHUNK];
        loop {
            let read = reader.read(&mut chunk).map_err(|error| match &origin {
                Some(path) => Error::Plan(format!("read source '{}': {error}", path.display())),
                None => Error::Plan(format!("read blob stream: {error}")),
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
        let handle = BlobHandle::new(sha, writer.digest())?;
        let dest = self.cache.join(handle.stored().hex());
        if driver::exists(&dest) {
            driver::remove_file(&staging).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        } else {
            driver::rename(&staging, &dest).map_err(|error| {
                Error::Plan(format!("cannot write '{}': {error}", dest.display()))
            })?;
        }
        Ok(handle)
    }

    /// Reports whether one stored hash reads present.
    ///
    /// Presence covers the cache and the pool.
    fn present(&self, stored: &Sha) -> bool {
        driver::exists(&self.cache.join(stored.hex()))
            || driver::exists(&self.pool.join(stored.hex()))
    }

    /// Reads the raw byte count for one handle.
    ///
    /// The count reads from the cache with fallback
    /// to the pool.
    ///
    /// # Errors
    ///
    /// Missing blobs fail as plan errors naming the hash.
    pub fn len(&self, handle: &BlobHandle) -> Result<u64> {
        pooled_len(&self.live_path(handle), handle.sha())
    }

    /// Persists cache blobs into the shared pool.
    ///
    /// # Errors
    ///
    /// Cache reads and pool write failures surface as plan
    /// or io errors.
    pub fn persist(&self, blobs: &[BlobHandle]) -> Result<()> {
        for blob in blobs {
            let dest = self.pool.join(blob.stored().hex());
            let source = self.cache.join(blob.stored().hex());
            if driver::exists(&dest) {
                // Cache copies clean best-effort; the OS reaps the rest.
                let _ = driver::remove_file(&source);
                continue;
            }
            if !driver::exists(&source) {
                continue;
            }
            if let Some(parent) = dest.parent() {
                driver::create_dir_all(parent).map_err(|error| {
                    Error::Plan(format!("cannot write '{}': {error}", dest.display()))
                })?;
            }
            match driver::rename(&source, &dest) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => {
                    stream_copy(&source, &dest, blob.sha())?;
                    // Cache copies clean best-effort; the OS reaps the rest.
                    let _ = driver::remove_file(&source);
                }
            }
        }
        Ok(())
    }

    /// Derives the live file for one handle.
    ///
    /// The cache wins while present.
    fn live_path(&self, handle: &BlobHandle) -> PathBuf {
        let cached = self.cache.join(handle.stored().hex());
        if driver::exists(&cached) {
            cached
        } else {
            self.pool.join(handle.stored().hex())
        }
    }

    /// Derives the live stored file for one stored hash.
    ///
    /// The cache wins while present.
    pub(crate) fn stored_path(&self, stored: &Sha) -> PathBuf {
        let cached = self.cache.join(stored.hex());
        if driver::exists(&cached) {
            cached
        } else {
            self.pool.join(stored.hex())
        }
    }

    /// Drops cache files and pool blobs unreferenced by slots and history.
    ///
    /// The returned count covers both homes.
    ///
    /// # Errors
    ///
    /// Listing and removal failures surface as plan or io errors.
    pub fn prune(&self) -> Result<usize> {
        let mut keep: BTreeSet<String> = BTreeSet::new();
        collect_manifest_refs(&self.config_base.join("state.json"), &mut keep);
        collect_dir_refs(&self.config_base.join("previous"), &mut keep)?;
        collect_dir_refs(&self.config_base.join("plans"), &mut keep)?;
        let mut removed = prune_dir(&self.cache, &keep)?;
        removed += prune_dir(&self.pool, &keep)?;
        Ok(removed)
    }
}

/// Streams one cache file into the pool without loading bytes.
///
/// # Errors
///
/// Cache reads and pool write failures surface as plan errors
/// naming the content hash or the destination.
fn stream_copy(source: &Path, dest: &Path, sha: &Sha) -> Result<()> {
    let mut reader = driver::open_read(source)
        .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?;
    let mut writer = driver::create(dest)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
    std::io::copy(&mut reader, &mut writer)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
    Ok(())
}

impl StoredWriter {
    /// Wraps one staging file with an encoded-bytes hash.
    fn new(file: Box<dyn std::io::Write>) -> Self {
        Self {
            file,
            hasher: sha2::Sha256::new(),
        }
    }

    /// Reads the encoded-bytes hash.
    fn digest(self) -> Sha {
        Sha::finish(self.hasher)
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
                let actual = Sha::finish(std::mem::replace(&mut self.hasher, sha2::Sha256::new()));
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
/// Reads the count from the gzip footer.
///
/// # Errors
///
/// Missing, short, and unreadable pool files fail as
/// plan errors naming the hash.
fn pooled_len(path: &Path, sha: &Sha) -> Result<u64> {
    let footer = driver::read_tail(path, GZIP_ISIZE_LEN).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::Plan(format!("missing blob '{sha}'"))
        } else if error.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::Plan(format!("read blob '{sha}': short pool file"))
        } else {
            Error::Plan(format!("read blob '{sha}': {error}"))
        }
    })?;
    let mut raw = [0u8; GZIP_ISIZE_LEN as usize];
    raw.copy_from_slice(&footer);
    Ok(u32::from_le_bytes(raw) as u64)
}

/// Drops files in one blob home unreferenced by the keep set.
///
/// Missing folders read as zero removals.
///
/// # Arguments
///
/// * `dir` - the blob home under sweeping.
/// * `keep` - the stored hashes under keeping.
///
/// # Returns
///
/// The count of dropped files.
///
/// # Errors
///
/// Listing and removal failures surface as plan or io errors.
fn prune_dir(dir: &Path, keep: &BTreeSet<String>) -> Result<usize> {
    let entries = match driver::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(Error::from(error)),
    };
    let mut removed = 0;
    for path in entries {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if keep.contains(&name) {
            continue;
        }
        driver::remove_file(&path).map_err(Error::from)?;
        removed += 1;
    }
    Ok(removed)
}

/// Collects blob refs from every manifest file in one folder.
///
/// Unreadable and unparsable files skip quietly.
///
/// # Errors
///
/// Listing failures surface as plan or io errors.
fn collect_dir_refs(dir: &Path, keep: &mut BTreeSet<String>) -> Result<()> {
    let files = match driver::read_dir(dir) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::from(error)),
    };
    for file in files {
        collect_manifest_refs(&file, keep);
    }
    Ok(())
}

/// Collects blob refs from one manifest file without hydrating.
///
/// Missing, unparsable, and stale files add no refs.
fn collect_manifest_refs(path: &Path, keep: &mut BTreeSet<String>) {
    let bytes = match driver::read(path) {
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
                .blob_refs()
                .into_iter()
                .map(|blob| blob.stored().hex()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{ManifestData, ManifestDocument};
    use confit_model::routes::{Route, RouteBase};

    use crate::bundle::Bundle;
    use crate::handles::FetchHandle;

    use crate::slot::SlotStore;
    use confit_driver::TestGuard;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            cache_base: dir.join("cache"),
            temp_base: dir.join("temp"),
        }
    }

    fn pool_entries(roots: &StoreRoots) -> Vec<PathBuf> {
        driver::read_dir(&roots.config_base.join("blobs")).unwrap_or_default()
    }

    fn test_store(dir: &Path) -> BlobStore {
        BlobStore::new(&test_roots(dir))
    }

    fn read_open(store: &BlobStore, handle: &BlobHandle) -> Vec<u8> {
        match store.open(handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                found
            }
            Err(error) => panic!("pooled bytes open: {error}"),
        }
    }

    #[test]
    fn put_open_round_trip_with_both_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = b"pooled bytes";
        let handle = match store.put(BlobSource::Bytes(raw)) {
            Ok(handle) => handle,
            Err(error) => panic!("pool stores: {error}"),
        };
        assert_eq!(handle.sha(), &Sha::hash(raw), "content hash seals bytes");
        assert_ne!(
            handle.stored(),
            handle.sha(),
            "stored hash seals encoded bytes"
        );
        assert!(
            store.resolve(&handle.to_ref()).is_ok(),
            "resolve proves the stored handle"
        );
        assert_eq!(read_open(&store, &handle), raw, "open round-trips bytes");
        match store.len(&handle) {
            Ok(len) => assert_eq!(len, raw.len() as u64, "length reads raw count"),
            Err(error) => panic!("pool length reads: {error}"),
        }
        match store.put(BlobSource::Bytes(raw)) {
            Ok(again) => assert_eq!(again, handle, "repeat put keeps identity"),
            Err(error) => panic!("repeat put stores: {error}"),
        }
    }

    #[test]
    fn put_spills_into_cache_with_zero_pool_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let store = BlobStore::new(&roots);
        let handle = match store.put(BlobSource::Bytes(b"preview bytes")) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores: {error}"),
        };
        assert!(
            pool_entries(&roots).is_empty(),
            "a preview writes zero pool bytes"
        );
        assert!(
            driver::read_dir(&roots.config_base)
                .unwrap_or_default()
                .is_empty(),
            "a preview writes zero config bytes with cache bytes allowed"
        );
        assert!(
            driver::exists(&roots.cache_base.join("blobs").join(handle.stored().hex())),
            "the handle points at its cache file"
        );
        assert_eq!(read_open(&store, &handle), b"preview bytes");
    }

    #[test]
    fn persist_lands_referenced_cache_blobs_in_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let store = BlobStore::new(&roots);
        let handle = match store.put(BlobSource::Bytes(b"applied bytes")) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores: {error}"),
        };
        assert!(
            pool_entries(&roots).is_empty(),
            "persist starts from zero pool bytes"
        );
        match store.persist(std::slice::from_ref(&handle)) {
            Ok(()) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        assert_eq!(pool_entries(&roots).len(), 1, "pool holds one file");
        assert!(
            !driver::exists(&roots.cache_base.join("blobs").join(handle.stored().hex())),
            "persist leaves no cache copy"
        );
        assert!(
            store.resolve(&handle.to_ref()).is_ok(),
            "resolve proves the persisted handle"
        );
        assert_eq!(read_open(&store, &handle), b"applied bytes");
        match store.persist(std::slice::from_ref(&handle)) {
            Ok(()) => {}
            Err(error) => panic!("repeat persist lands: {error}"),
        }
        assert!(
            store.resolve(&handle.to_ref()).is_ok(),
            "resolve proves past repeat persist"
        );
    }

    #[test]
    fn cache_wins_while_present() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let store = BlobStore::new(&roots);
        let raw = b"cache precedence bytes";
        let handle = match store.put(BlobSource::Bytes(raw)) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores: {error}"),
        };
        match store.persist(std::slice::from_ref(&handle)) {
            Ok(()) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        let again = match store.put(BlobSource::Bytes(raw)) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores again: {error}"),
        };
        assert_eq!(again, handle, "repeat put keeps identity");
        let cached = roots.cache_base.join("blobs").join(handle.stored().hex());
        let pooled = roots.config_base.join("blobs").join(handle.stored().hex());
        assert!(
            driver::exists(&cached) && driver::exists(&pooled),
            "both homes hold the blob"
        );
        assert_eq!(read_open(&store, &handle), raw, "both homes read bytes");
        driver::remove_file(&cached).unwrap();
        assert_eq!(
            read_open(&store, &handle),
            raw,
            "pool serves past cache loss"
        );
        let remade = match store.put(BlobSource::Bytes(raw)) {
            Ok(handle) => handle,
            Err(error) => panic!("cache stores after loss: {error}"),
        };
        assert_eq!(remade, handle, "remade put keeps identity");
        driver::remove_file(&pooled).unwrap();
        assert!(
            store.resolve(&handle.to_ref()).is_ok(),
            "resolve proves from cache alone"
        );
        assert_eq!(read_open(&store, &handle), raw, "cache wins while present");
    }

    #[test]
    fn missing_blob_names_hash() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let handle = BlobHandle::new(Sha::hash(b"absent"), Sha::hash(b"absent-stored")).unwrap();
        assert!(
            store.resolve(&handle.to_ref()).is_err(),
            "absent ref never resolves"
        );
        match store.open(&handle) {
            Ok(_) => panic!("absent blob opens"),
            Err(error) => assert!(
                error.to_string().contains(&handle.sha().hex()),
                "missing open names the hash: {error}"
            ),
        }
        match store.len(&handle) {
            Ok(_) => panic!("absent length reads"),
            Err(error) => assert!(
                error.to_string().contains(&handle.sha().hex()),
                "missing length names the hash: {error}"
            ),
        }
    }

    #[test]
    fn both_source_shapes_keep_sealed_identity() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = b"shared pool bytes";
        let via_put = match store.put(BlobSource::Bytes(raw)) {
            Ok(handle) => handle,
            Err(error) => panic!("pool stores: {error}"),
        };
        let source_path = dir.path().join("source.bin");
        driver::write(&source_path, raw).unwrap();
        let source =
            FetchHandle::new(source_path, Sha::hash(raw), "https://example.com/source").unwrap();
        match store.put(BlobSource::Handle(&source)) {
            Ok(via_source) => assert_eq!(via_source, via_put, "source keeps identity"),
            Err(error) => panic!("source stores: {error}"),
        }
        assert_eq!(read_open(&store, &via_put), raw);
        let missing_path = dir.path().join("absent.bin");
        let missing = FetchHandle::new(
            missing_path.clone(),
            Sha::hash(b"x"),
            "https://example.com/x",
        )
        .unwrap();
        match store.put(BlobSource::Handle(&missing)) {
            Ok(_) => panic!("absent source passes"),
            Err(error) => assert!(
                error
                    .to_string()
                    .contains(&missing_path.display().to_string()),
                "absent source names the path: {error}"
            ),
        }
    }

    #[test]
    fn prune_keeps_referenced_pool_entries() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let store = BlobStore::new(&roots);
        let slots = SlotStore::new(&roots);
        let kept = match store.put(BlobSource::Bytes(b"kept bytes")) {
            Ok(handle) => handle,
            Err(error) => panic!("kept blob stores: {error}"),
        };
        let dropped = match store.put(BlobSource::Bytes(b"dropped bytes")) {
            Ok(handle) => handle,
            Err(error) => panic!("dropped blob stores: {error}"),
        };
        let mut bundle = match Bundle::build(
            vec![ManifestDocument::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                ManifestData::Opaque {
                    blob: kept.to_ref(),
                    size: 10,
                    mode: None,
                    unmanaged: false,
                },
            )],
            Vec::new(),
        ) {
            Ok(bundle) => bundle,
            Err(error) => panic!("bundle builds: {error}"),
        };
        bundle.blobs.insert(kept.sha().hex(), kept.to_ref());
        match store.persist(std::slice::from_ref(&kept)) {
            Ok(_) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        match slots.store(&bundle, None) {
            Ok(_) => {}
            Err(error) => panic!("referencing bundle stores: {error}"),
        }
        match store.prune() {
            Ok(removed) => assert_eq!(removed, 1, "prune drops one entry"),
            Err(error) => panic!("pool prunes: {error}"),
        }
        assert!(
            store.resolve(&kept.to_ref()).is_ok(),
            "resolve proves the referenced blob"
        );
        assert!(
            store.resolve(&dropped.to_ref()).is_err(),
            "resolve refuses the dropped blob"
        );
        assert_eq!(read_open(&store, &kept), b"kept bytes");
        for _ in 1..=6 {
            match slots.store(&Bundle::empty(), None) {
                Ok(_) => {}
                Err(error) => panic!("empty bundle stores: {error}"),
            }
        }
        match store.prune() {
            Ok(removed) => assert_eq!(removed, 1, "flushed state drops the rest"),
            Err(error) => panic!("pool prunes again: {error}"),
        }
        assert!(
            store.resolve(&kept.to_ref()).is_err(),
            "resolve refuses the dangling blob"
        );
    }
}
