//! Blob
//!
//! Content-addressed blob pool behind stored hashes.

pub mod error;

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use confit_model::document::BlobRef;
use confit_model::sha::{Sha, ShaWriter};
use sha2::Digest;

use crate::StoreRoots;
use crate::handles::{BlobHandle, TrustedHandle};
use crate::slot::SlotStore;
use confit_driver::{self as driver};
use error::{BlobError, Result};

/// Pool folder name under the config base.
const BLOBS_DIR: &str = "blobs";

/// Pool gzip level.
const BLOB_GZIP_LEVEL: u32 = 6;

/// Content-addressed blob pool.
///
/// Plan cache files ride `{cache}/blobs/{stored}` as gzip bytes.
/// Named pool files ride `{config}/blobs/{stored}` as gzip bytes.
/// Spill verbs write into the cache. The pool fills
/// through persist alone at apply end.
#[derive(Debug, Clone)]
pub struct BlobStore {
    roots: StoreRoots,
    pool: PathBuf,
    cache: PathBuf,
}

/// Verifying blob byte stream.
struct VerifiedBlobReader {
    decoder: driver::gzip::Decoder<Box<dyn std::io::Read>>,
    sha: Sha,
    hasher: sha2::Sha256,
    done: bool,
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
            roots: roots.clone(),
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
    /// - [`BlobError::Missing`] for missing blobs.
    pub fn resolve(&self, blob: &BlobRef) -> Result<BlobHandle> {
        if self.present(blob.stored()) {
            BlobHandle::new(blob.sha().clone(), blob.stored().clone()).map_err(|_| {
                BlobError::Corrupt {
                    sha: blob.sha().clone(),
                }
            })
        } else {
            Err(BlobError::Missing {
                sha: blob.sha().clone(),
            })
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
    /// - [`BlobError::Missing`] for missing blobs.
    /// - [`BlobError::Denied`] for denied blobs.
    /// - [`BlobError::Unknown`] for other failures.
    pub fn open(&self, handle: &BlobHandle) -> Result<Box<dyn std::io::Read>> {
        let sha = handle.sha().clone();
        let path = self.live_path(handle);
        let file = driver::fs::open(&path).map_err(|error| BlobError::from_read_io(&sha, error))?;
        Ok(Box::new(VerifiedBlobReader {
            decoder: driver::gzip::Decoder::open(file as Box<dyn std::io::Read>),
            sha,
            hasher: sha2::Sha256::new(),
            done: false,
        }) as Box<dyn std::io::Read>)
    }

    /// Spills bytes into the cache sealing both identities.
    ///
    /// Staging keys on the content hash since the stored name
    /// arrives only after bytes land, so the first phase stages
    /// by hand and the close renames straight into place. Same
    /// stored names hold same bytes, so the rename needs no
    /// present-wins race.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Missing`] for missing sources.
    /// - [`BlobError::Denied`] for denied sources.
    /// - [`BlobError::Unknown`] for other read failures.
    /// - [`BlobError::WriteMissing`] for missing cache paths.
    /// - [`BlobError::WriteDenied`] for denied cache paths.
    /// - [`BlobError::WriteUnknown`] for other cache write failures.
    /// - [`BlobError::Compress`] for compression failures.
    pub fn put(&self, source: BlobSource<'_>) -> Result<BlobHandle> {
        let (mut reader, sha): (Box<dyn Read>, Sha) = match source {
            BlobSource::Bytes(bytes) => (
                Box::new(std::io::Cursor::new(bytes)) as Box<dyn Read>,
                Sha::hash(bytes),
            ),
            BlobSource::Handle(handle) => {
                let path = handle.canonical();
                let sha = handle.sha().clone();
                let file =
                    driver::fs::open(path).map_err(|error| BlobError::from_read_io(&sha, error))?;
                (file as Box<dyn Read>, sha)
            }
        };
        driver::fs::create_dir_all(&self.cache)
            .map_err(|error| BlobError::from_write_io(&self.cache, error))?;
        let staging = driver::stage_path(&self.cache.join(sha.hex()));
        let staged = driver::fs::create(&staging)
            .map_err(|error| BlobError::from_write_io(&staging, error))?;
        let mut encoder = driver::gzip::Encoder::open(ShaWriter::new(staged), BLOB_GZIP_LEVEL);
        driver::copy_stream(&mut reader, &mut encoder).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
                BlobError::from_read_io(&sha, error)
            }
            _ => BlobError::Compress,
        })?;
        let writer = encoder.finish().map_err(|_| BlobError::Compress)?;
        let stored = writer.digest();
        let handle =
            BlobHandle::new(sha.clone(), stored).map_err(|_| BlobError::Corrupt { sha })?;
        let dest = self.cache.join(handle.stored().hex());
        driver::fs::rename(&staging, &dest)
            .map_err(|error| BlobError::from_write_io(&dest, error))?;
        Ok(handle)
    }

    /// Reports whether one stored hash reads present.
    ///
    /// Presence covers the cache and the pool.
    fn present(&self, stored: &Sha) -> bool {
        driver::fs::exists(&self.cache.join(stored.hex()))
            || driver::fs::exists(&self.pool.join(stored.hex()))
    }

    /// Lands already-compressed bytes from a trusted member into the cache.
    ///
    /// Presence wins and proof rides lazy at open.
    ///
    /// # Errors
    ///
    /// - [`BlobError::WriteMissing`] for missing cache paths.
    /// - [`BlobError::WriteDenied`] for denied cache paths.
    /// - [`BlobError::WriteUnknown`] for other cache write failures.
    pub(crate) fn receive(&self, stored: &Sha, source: &dyn TrustedHandle) -> Result<()> {
        let dest = self.cache.join(stored.hex());
        if driver::fs::exists(&dest) {
            return Ok(());
        }
        driver::atomic_write(&dest, |staging| {
            driver::fs::copy(source.canonical(), staging).map(|_| ())
        })
        .map_err(|error| BlobError::from_write_io(&dest, error))
    }

    /// Opens raw stored bytes with their encoded length.
    ///
    /// Reads ride the stored hash from the cache with
    /// fallback to the pool. Bytes stay encoded for
    /// verbatim egress.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Missing`] for missing blobs.
    /// - [`BlobError::Denied`] for denied blobs.
    /// - [`BlobError::Unknown`] for other failures.
    pub(crate) fn open_stored(&self, handle: &BlobHandle) -> Result<(u64, Box<dyn std::io::Read>)> {
        let path = self.live_path(handle);
        let len = driver::fs::metadata(&path)
            .map_err(|error| BlobError::from_read_io(handle.sha(), error))?
            .len();
        let file = driver::fs::open(&path)
            .map_err(|error| BlobError::from_read_io(handle.sha(), error))?;
        Ok((len, file as Box<dyn std::io::Read>))
    }

    /// Reads the raw byte count for one handle.
    ///
    /// The count reads from the cache with fallback
    /// to the pool.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Missing`] for missing blobs.
    /// - [`BlobError::Denied`] for denied blobs.
    /// - [`BlobError::Unknown`] for other failures.
    /// - [`BlobError::Corrupt`] for corrupt pool files.
    pub fn len(&self, handle: &BlobHandle) -> Result<u64> {
        let sha = handle.sha().clone();
        let footer = driver::fs::read_tail(&self.live_path(handle), driver::gzip::TRAILER_LEN)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => BlobError::Missing { sha: sha.clone() },
                std::io::ErrorKind::PermissionDenied => BlobError::Denied { sha: sha.clone() },
                std::io::ErrorKind::UnexpectedEof => BlobError::Corrupt { sha: sha.clone() },
                _ => BlobError::from_read_io(&sha, error),
            })?;
        let mut raw = [0u8; driver::gzip::TRAILER_LEN as usize];
        raw.copy_from_slice(&footer);
        Ok(u32::from_le_bytes(raw) as u64)
    }

    /// Persists cache blobs into the shared pool.
    ///
    /// The cache copy cleans best effort once the pool holds
    /// the bytes, and a cache file vanishing mid-run skips.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Missing`] for missing blobs.
    /// - [`BlobError::Denied`] for denied blobs.
    /// - [`BlobError::Unknown`] for other cache read failures.
    /// - [`BlobError::WriteMissing`] for missing pool paths.
    /// - [`BlobError::WriteDenied`] for denied pool paths.
    /// - [`BlobError::WriteUnknown`] for other pool write failures.
    pub fn persist(&self, blobs: &[BlobHandle]) -> Result<()> {
        for blob in blobs {
            let dest = self.pool.join(blob.stored().hex());
            let source = self.cache.join(blob.stored().hex());
            if driver::fs::exists(&dest) {
                // Cache copies clean best-effort; the OS reaps the rest.
                let _ = driver::fs::remove_file(&source);
                continue;
            }
            if !driver::fs::exists(&source) {
                continue;
            }
            match driver::atomic_write(&dest, |staging| move_cache_copy(&source, staging)) {
                Ok(()) => {
                    // Cache copies clean best-effort; the OS reaps the rest.
                    let _ = driver::fs::remove_file(&source);
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(BlobError::from_write_io(&dest, error)),
            }
        }
        Ok(())
    }

    /// Derives the live file for one handle.
    ///
    /// The cache wins while present.
    fn live_path(&self, handle: &BlobHandle) -> PathBuf {
        let cached = self.cache.join(handle.stored().hex());
        if driver::fs::exists(&cached) {
            cached
        } else {
            self.pool.join(handle.stored().hex())
        }
    }

    /// Drops cache and pool files outside what slot records.
    ///
    /// The sweep protects what slot records, read through one
    /// slot view built from the same roots, and the removal
    /// count covers both homes.
    ///
    /// # Errors
    ///
    /// - [`BlobError::WriteMissing`] for missing paths.
    /// - [`BlobError::WriteDenied`] for denied paths.
    /// - [`BlobError::WriteUnknown`] for keep set reads, listing,
    ///   and removal failures.
    pub fn prune(&self) -> Result<usize> {
        let slots = SlotStore::new(&self.roots);
        let keep = slots.keep_set().map_err(|error| BlobError::WriteUnknown {
            path: self.roots.config_base.clone(),
            message: error.to_string(),
        })?;
        let mut removed = prune_dir(&self.cache, &keep)?;
        removed += prune_dir(&self.pool, &keep)?;
        Ok(removed)
    }
}

/// Moves one cache file into its pool staging file.
///
/// A cross-device rename falls back to a verbatim copy.
///
/// # Errors
///
/// - io failures from rename, open, and copy carry the cache file.
/// - a vanished cache file reports as NotFound.
fn move_cache_copy(source: &Path, staging: &Path) -> std::io::Result<()> {
    match driver::fs::rename(source, staging) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(error),
        Err(_) => {
            let mut reader = driver::fs::open(source)?;
            let mut writer = driver::fs::create(staging)?;
            driver::copy_stream(&mut reader, &mut writer)?;
            Ok(())
        }
    }
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
                    let message = BlobError::Corrupt {
                        sha: self.sha.clone(),
                    }
                    .to_string();
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        message,
                    ));
                }
                Ok(0)
            }
            Ok(read) => {
                self.hasher.update(&buf[..read]);
                Ok(read)
            }
            Err(error) => {
                let kind = error.kind();
                let message = error.to_string();
                Err(std::io::Error::new(kind, message))
            }
        }
    }
}

/// Drops files in one blob home outside the keep set.
///
/// Missing folders read as zero removals.
///
/// # Errors
///
/// - [`BlobError::WriteMissing`] for missing paths.
/// - [`BlobError::WriteDenied`] for denied paths.
/// - [`BlobError::WriteUnknown`] for other listing and removal failures.
fn prune_dir(dir: &Path, keep: &BTreeSet<Sha>) -> Result<usize> {
    let entries = match driver::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => {
            return Err(BlobError::from_write_io(dir, error));
        }
    };
    let mut removed = 0;
    for path in entries {
        if held(&path, keep) {
            continue;
        }
        driver::fs::remove_file(&path).map_err(|error| BlobError::from_write_io(&path, error))?;
        removed += 1;
    }
    Ok(removed)
}

/// Reports whether one blob home file names a hash inside the keep set.
///
/// Names failing hash parsing read outside the set.
fn held(path: &Path, keep: &BTreeSet<Sha>) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .and_then(|name| Sha::new(name).ok())
        .is_some_and(|stored| keep.contains(&stored))
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{Data, Document};
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};

    use crate::handles::FetchHandle;

    use confit_driver::fs::TestGuard;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            cache_base: dir.join("cache"),
            temp_base: dir.join("temp"),
        }
    }

    fn pool_entries(roots: &StoreRoots) -> Vec<PathBuf> {
        driver::fs::read_dir(&roots.config_base.join("blobs")).unwrap_or_default()
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
            driver::fs::read_dir(&roots.config_base)
                .unwrap_or_default()
                .is_empty(),
            "a preview writes zero config bytes with cache bytes allowed"
        );
        assert!(
            driver::fs::exists(&roots.cache_base.join("blobs").join(handle.stored().hex())),
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
            !driver::fs::exists(&roots.cache_base.join("blobs").join(handle.stored().hex())),
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
            driver::fs::exists(&cached) && driver::fs::exists(&pooled),
            "both homes hold the blob"
        );
        assert_eq!(read_open(&store, &handle), raw, "both homes read bytes");
        driver::fs::remove_file(&cached).unwrap();
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
        driver::fs::remove_file(&pooled).unwrap();
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
        driver::fs::write(&source_path, raw).unwrap();
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
            Err(BlobError::Missing { sha }) => {
                assert_eq!(sha, Sha::hash(b"x"), "absent source keeps the content hash")
            }
            Err(error) => panic!("wrong absent variant: {error}"),
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
        let manifest = match Manifest::build(
            vec![Document::new(
                Route::new(RouteBase::Home, "bin").unwrap(),
                Data::Opaque {
                    blob: kept.to_ref(),
                    size: 10,
                    mode: None,
                    unmanaged: false,
                },
            )],
            Vec::new(),
        ) {
            Ok(manifest) => manifest,
            Err(error) => panic!("manifest builds: {error}"),
        };
        match store.persist(std::slice::from_ref(&kept)) {
            Ok(_) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        match slots.store(&manifest, None) {
            Ok(_) => {}
            Err(error) => panic!("referencing manifest stores: {error}"),
        }
        match store.prune() {
            Ok(removed) => assert_eq!(removed, 1, "prune drops the orphan"),
            Err(error) => panic!("pool prunes: {error}"),
        }
        assert!(
            store.resolve(&kept.to_ref()).is_ok(),
            "the referenced blob survives"
        );
        assert!(store.resolve(&dropped.to_ref()).is_err(), "the orphan dies");
        assert_eq!(read_open(&store, &kept), b"kept bytes");
        for _ in 1..=6 {
            match slots.store(&Manifest::empty(), None) {
                Ok(_) => {}
                Err(error) => panic!("empty manifest stores: {error}"),
            }
        }
        match store.prune() {
            Ok(removed) => assert_eq!(removed, 1, "rotation drains the rest"),
            Err(error) => panic!("pool prunes again: {error}"),
        }
        assert!(
            store.resolve(&kept.to_ref()).is_err(),
            "resolve refuses the dangling blob"
        );
    }
}
