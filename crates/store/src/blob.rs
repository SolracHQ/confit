//! Content-addressed blob pool.

pub mod error;
mod file;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use confit_driver::progress::{Event, ProgressSender};
use confit_driver::sha::Sha;

use crate::StoreRoots;
use crate::faults::AccessFault;
use crate::handles::{BlobHandle, ResourceHandle};
use confit_driver::{self as driver};
use error::{BlobError, Result};
use file::BlobFile;

/// Pool folder name under the config base.
const BLOBS_DIR: &str = "blobs";

/// Content-addressed blob pool.
#[derive(Debug, Clone)]
pub struct BlobStore {
    pool: PathBuf,
    cache: PathBuf,
    progress: ProgressSender,
    finished: Arc<AtomicUsize>,
}

impl BlobStore {
    /// Build one file-backed blob pool with pending under temp.
    pub fn new(roots: &StoreRoots, progress: ProgressSender) -> Self {
        Self {
            pool: roots.config_base.join(BLOBS_DIR),
            cache: roots.temp_base.join(BLOBS_DIR),
            progress,
            finished: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Resolve one content hash into its handle.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for missing blobs.
    pub fn resolve(&self, sha: &Sha) -> Result<BlobHandle> {
        if self.present(sha) {
            Ok(BlobHandle::new(sha.clone()))
        } else {
            Err(BlobError::Read {
                sha: sha.clone(),
                fault: AccessFault::Missing,
            })
        }
    }

    /// Open a raw byte stream for one handle.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for failed blob reads.
    /// - [`BlobError::Corrupt`] for bad headers.
    pub fn open(&self, handle: &BlobHandle) -> Result<Box<dyn std::io::Read>> {
        let path = self.current_path(handle);
        BlobFile::open(path, handle.sha256())?.body()
    }

    /// Write one project file into the cache under its content hash.
    ///
    /// - Present blobs skip compression.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for failed source reads.
    /// - [`BlobError::Write`] for cache write faults.
    /// - [`BlobError::WriteUnknown`] for other cache write failures.
    /// - [`BlobError::Compress`] for compression failures.
    pub fn put(&self, handle: &ResourceHandle) -> Result<BlobHandle> {
        let (digest, size) = describe_source(handle)?;
        if let Some(found) = self.reuse(&digest) {
            return Ok(found);
        }
        let path = handle.canonical();
        let mut reader =
            driver::fs::open(path).map_err(|error| BlobError::from_read_io(&digest, error))?;
        driver::fs::create_dir_all(&self.cache)
            .map_err(|error| BlobError::from_write_io(&self.cache, error))?;
        let pending = driver::pending_path(&self.cache.join(digest.hex()));
        let sink = driver::fs::create(&pending)
            .map_err(|error| BlobError::from_write_io(&pending, error))?;
        let mut writer = BlobFile::create(sink, &digest, size)
            .map_err(|error| BlobError::from_write_io(&pending, error))?;
        driver::copy_stream(&mut reader, &mut writer).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied => {
                BlobError::from_read_io(&digest, error)
            }
            _ => BlobError::Compress,
        })?;
        writer
            .finish()
            .map_err(|error| BlobError::from_write_io(&pending, error))?;
        let handle = BlobHandle::new(digest.clone());
        let dest = self.cache.join(digest.hex());
        driver::fs::rename(&pending, &dest)
            .map_err(|error| BlobError::from_write_io(&dest, error))?;
        let done = self.finished.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.progress.send(Event::BlobCompressed { done });
        Ok(handle)
    }

    /// Report whether one content hash is present in the cache or pool.
    fn present(&self, sha: &Sha) -> bool {
        driver::fs::exists(&self.cache.join(sha.hex()))
            || driver::fs::exists(&self.pool.join(sha.hex()))
    }

    /// Reuse one present blob without compression.
    fn reuse(&self, sha: &Sha) -> Option<BlobHandle> {
        if !self.present(sha) {
            return None;
        }
        Some(BlobHandle::new(sha.clone()))
    }

    /// Receive one object member into the cache under its content hash.
    ///
    /// - Present entries skip the copy.
    /// - Member headers match the received hash.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Corrupt`] for member and hash mismatches.
    /// - [`BlobError::Read`] for failed member reads.
    /// - [`BlobError::Write`] for cache write faults.
    /// - [`BlobError::WriteUnknown`] for other cache write failures.
    pub fn receive(&self, sha: &Sha, source: &ResourceHandle) -> Result<()> {
        BlobFile::open(source.canonical().to_path_buf(), sha)?;
        let dest = self.cache.join(sha.hex());
        if driver::fs::exists(&dest) {
            return Ok(());
        }
        driver::atomic_write(&dest, |pending| {
            driver::fs::copy(source.canonical(), pending).map(|_| ())
        })
        .map_err(|error| BlobError::from_write_io(&dest, error))
    }

    /// Open raw object bytes with their encoded length.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for failed blob reads.
    /// - [`BlobError::Corrupt`] for bad headers.
    pub fn open_object(&self, handle: &BlobHandle) -> Result<(u64, Box<dyn std::io::Read>)> {
        let path = self.current_path(handle);
        let file = BlobFile::open(path, handle.sha256())?;
        file.raw()
    }

    /// Read the content byte count from the object header.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for failed blob reads.
    /// - [`BlobError::Corrupt`] for bad headers.
    pub fn len(&self, handle: &BlobHandle) -> Result<u64> {
        let path = self.current_path(handle);
        Ok(BlobFile::open(path, handle.sha256())?.size())
    }

    /// Persist cache blobs into the shared pool.
    ///
    /// - Vanished cache files skip.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Read`] for failed cache reads.
    /// - [`BlobError::Write`] for pool write faults.
    /// - [`BlobError::WriteUnknown`] for other pool write failures.
    pub fn persist(&self, blobs: &[BlobHandle]) -> Result<()> {
        if !blobs.is_empty() {
            let _ = self.progress.send(Event::Promoting { blobs: blobs.len() });
        }
        for blob in blobs {
            let dest = self.pool.join(blob.sha256().hex());
            let source = self.cache.join(blob.sha256().hex());
            if driver::fs::exists(&dest) {
                // Cache copies clean best-effort; the OS reaps the rest.
                let _ = driver::fs::remove_file(&source);
                continue;
            }
            if driver::fs::is_missing(&source) {
                continue;
            }
            match driver::atomic_write(&dest, |pending| move_cache_copy(&source, pending)) {
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

    /// Derive the current file for one handle.
    ///
    /// - The cache copy wins over the pool.
    fn current_path(&self, handle: &BlobHandle) -> PathBuf {
        let cached = self.cache.join(handle.sha256().hex());
        if driver::fs::exists(&cached) {
            cached
        } else {
            self.pool.join(handle.sha256().hex())
        }
    }

    /// Drop cache and pool files outside the keep set.
    ///
    /// The removal count for both homes.
    ///
    /// # Errors
    ///
    /// - [`BlobError::Write`] for write faults.
    /// - [`BlobError::WriteUnknown`] for listing and removal failures.
    pub fn prune(&self, keep: &BTreeSet<Sha>) -> Result<usize> {
        let mut removed = prune_dir(&self.cache, keep)?;
        removed += prune_dir(&self.pool, keep)?;
        Ok(removed)
    }
}

/// Move one cache file into its pool pending file.
///
/// - A cross-device rename falls back to an exact copy.
///
/// # Errors
///
/// - io failures from rename, open, and copy carry the cache file.
/// - a vanished cache file reports as NotFound.
fn move_cache_copy(source: &Path, pending: &Path) -> std::io::Result<()> {
    match driver::fs::rename(source, pending) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(error),
        Err(_) => {
            let mut reader = driver::fs::open(source)?;
            let mut writer = driver::fs::create(pending)?;
            driver::copy_stream(&mut reader, &mut writer)?;
            Ok(())
        }
    }
}

/// Measure one project file into its content hash and byte count.
///
/// # Errors
///
/// - [`BlobError::Read`] for failed source stats.
fn describe_source(handle: &ResourceHandle) -> Result<(Sha, u64)> {
    let digest = handle.sha256().clone();
    let size = driver::fs::metadata(handle.canonical())
        .map_err(|error| BlobError::from_read_io(&digest, error))?
        .len();
    Ok((digest, size))
}

/// Drop files in one blob home outside the keep set.
///
/// - Missing folders are zero removals.
///
/// # Errors
///
/// - [`BlobError::Write`] for write faults.
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

/// Report whether one blob home file is inside the keep set.
///
/// - Files failing hash parsing are outside the set.
fn held(path: &Path, keep: &BTreeSet<Sha>) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .and_then(|name| Sha::new(name).ok())
        .is_some_and(|content| keep.contains(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Read as _;

    use crate::handles::ResourceHandle;

    use confit_driver::fs::TestGuard;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        }
    }

    fn pool_entries(roots: &StoreRoots) -> Vec<PathBuf> {
        driver::fs::read_dir(&roots.config_base.join("blobs")).unwrap_or_default()
    }

    fn test_store(dir: &Path) -> BlobStore {
        let (sender, _) = crossbeam_channel::unbounded();
        BlobStore::new(&test_roots(dir), sender)
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

    fn seed_file(dir: &Path, name: &str, body: &[u8]) {
        let project = dir.join("project");
        confit_driver::fs::create_dir_all(&project).unwrap();
        confit_driver::fs::write(&project.join(name), body).unwrap();
    }

    fn source_of(dir: &Path, name: &str, body: &[u8]) -> ResourceHandle {
        let project = dir.join("project");
        match ResourceHandle::new(&project, project.join(name), Sha::hash(body)) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        }
    }

    fn put_bytes(store: &BlobStore, dir: &Path, name: &str, body: &[u8]) -> BlobHandle {
        seed_file(dir, name, body);
        match store.put(&source_of(dir, name, body)) {
            Ok(handle) => handle,
            Err(error) => panic!("pool stores: {error}"),
        }
    }

    #[test]
    fn put_open_round_trip_with_content_names() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let raw = b"pooled bytes";
        let handle = put_bytes(&store, dir.path(), "pooled.bin", raw);
        assert_eq!(
            handle.sha256(),
            &Sha::hash(raw),
            "content hash matches bytes"
        );
        assert!(
            driver::fs::exists(&roots.temp_base.join("blobs").join(handle.sha256().hex())),
            "the file name carries the content hash"
        );
        assert!(
            store.resolve(handle.sha256()).is_ok(),
            "resolve proves the content handle"
        );
        assert_eq!(read_open(&store, &handle), raw, "open round-trips bytes");
        match store.len(&handle) {
            Ok(len) => assert_eq!(len, raw.len() as u64, "length is raw count"),
            Err(error) => panic!("pool length reads: {error}"),
        }
        let again = put_bytes(&store, dir.path(), "pooled.bin", raw);
        assert_eq!(again, handle, "repeat put keeps identity");
    }

    #[test]
    fn put_extracts_into_cache_with_zero_pool_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let handle = put_bytes(&store, dir.path(), "preview.bin", b"preview bytes");
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
            driver::fs::exists(&roots.temp_base.join("blobs").join(handle.sha256().hex())),
            "the handle points at its cache file"
        );
        assert_eq!(read_open(&store, &handle), b"preview bytes");
    }

    #[test]
    fn persist_lands_referenced_cache_blobs_in_pool() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let handle = put_bytes(&store, dir.path(), "applied.bin", b"applied bytes");
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
            !driver::fs::exists(&roots.temp_base.join("blobs").join(handle.sha256().hex())),
            "persist leaves no cache copy"
        );
        assert!(
            store.resolve(handle.sha256()).is_ok(),
            "resolve proves the persisted handle"
        );
        assert_eq!(read_open(&store, &handle), b"applied bytes");
        match store.persist(std::slice::from_ref(&handle)) {
            Ok(()) => {}
            Err(error) => panic!("repeat persist lands: {error}"),
        }
        assert!(
            store.resolve(handle.sha256()).is_ok(),
            "resolve proves past repeat persist"
        );
    }

    #[test]
    fn cache_wins_while_present() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let raw = b"cache precedence bytes";
        let handle = put_bytes(&store, dir.path(), "cache.bin", raw);
        match store.persist(std::slice::from_ref(&handle)) {
            Ok(()) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        let again = put_bytes(&store, dir.path(), "cache.bin", raw);
        assert_eq!(again, handle, "repeat put keeps identity");
        let pooled = roots.config_base.join("blobs").join(handle.sha256().hex());
        assert!(driver::fs::exists(&pooled), "pool holds the blob");
        assert_eq!(read_open(&store, &handle), raw, "pool reads bytes");
        assert_eq!(
            read_open(&store, &handle),
            raw,
            "pool serves past cache loss"
        );
        let remade = put_bytes(&store, dir.path(), "cache.bin", raw);
        assert_eq!(remade, handle, "remade put keeps identity");
        driver::fs::remove_file(&pooled).unwrap();
        let healed = put_bytes(&store, dir.path(), "cache.bin", raw);
        assert_eq!(healed, handle, "absent blobs recompress");
        let cached = roots.temp_base.join("blobs").join(handle.sha256().hex());
        assert!(driver::fs::exists(&cached), "recompress lands cache");
        assert!(
            store.resolve(handle.sha256()).is_ok(),
            "resolve proves from cache alone"
        );
        assert_eq!(read_open(&store, &handle), raw, "cache wins while present");
    }

    #[test]
    fn put_skips_present_blobs_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let raw = b"skipped bytes";
        let handle = put_bytes(&store, dir.path(), "skipped.bin", raw);
        guard.fail_writes(std::io::ErrorKind::QuotaExceeded);
        match store.put(&source_of(dir.path(), "skipped.bin", raw)) {
            Ok(again) => assert_eq!(again, handle, "repeat put keeps identity"),
            Err(error) => panic!("present put skips writes: {error}"),
        }
    }

    #[test]
    fn repeat_puts_share_one_file() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let raw = b"shared file bytes";
        let first = put_bytes(&store, dir.path(), "shared.bin", raw);
        let again = put_bytes(&store, dir.path(), "shared.bin", raw);
        assert_eq!(again, first, "second put keeps identity");
        match driver::fs::read_dir(&roots.temp_base.join("blobs")) {
            Ok(files) => assert_eq!(files.len(), 1, "one file serves both puts"),
            Err(error) => panic!("cache lists: {error}"),
        }
    }

    #[test]
    fn bad_body_fails_reads() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let raw = b"checked bytes";
        let handle = put_bytes(&store, dir.path(), "checked.bin", raw);
        let path = roots.temp_base.join("blobs").join(handle.sha256().hex());
        let mut body = match driver::fs::read(&path) {
            Ok(found) => found,
            Err(error) => panic!("object reads: {error}"),
        };
        let split = std::mem::size_of::<file::ObjectHeader>();
        body[split] ^= 0xff;
        driver::fs::write(&path, &body).unwrap();
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                if reader.read_to_end(&mut found).is_ok() {
                    panic!("bad body opens");
                }
            }
            Err(error) => panic!("bad header opens: {error}"),
        }
        assert!(driver::fs::exists(&path), "bad bodies stay for retry");
    }

    #[test]
    fn tampered_hash_reports_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let handle = put_bytes(&store, dir.path(), "staled.bin", b"staled bytes");
        let path = roots.temp_base.join("blobs").join(handle.sha256().hex());
        let body = match driver::fs::read(&path) {
            Ok(found) => found,
            Err(error) => panic!("object reads: {error}"),
        };
        let split = std::mem::size_of::<file::ObjectHeader>();
        let mut header = match bytemuck::try_from_bytes::<file::ObjectHeader>(&body[..split]) {
            Ok(header) => *header,
            Err(error) => panic!("header decodes: {error}"),
        };
        header.sha[0] ^= 0xff;
        let mut tampered = bytemuck::bytes_of(&header).to_vec();
        tampered.extend_from_slice(&body[split..]);
        driver::fs::write(&path, &tampered).unwrap();
        match store.len(&handle) {
            Ok(_) => panic!("tampered hash reads"),
            Err(BlobError::Corrupt { sha: found }) => {
                assert_eq!(found, *handle.sha256(), "tampered files keep the hash")
            }
            Err(error) => panic!("wrong tampered variant: {error}"),
        }
        assert!(driver::fs::exists(&path), "tampered files stay for retry");
    }

    #[test]
    fn truncated_body_fails_reads() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let handle = put_bytes(&store, dir.path(), "truncated.bin", b"truncated bytes");
        let path = roots.temp_base.join("blobs").join(handle.sha256().hex());
        let body = match driver::fs::read(&path) {
            Ok(found) => found,
            Err(error) => panic!("object reads: {error}"),
        };
        driver::fs::write(
            &path,
            &body[..std::mem::size_of::<file::ObjectHeader>() + 8],
        )
        .unwrap();
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                if reader.read_to_end(&mut found).is_ok() {
                    panic!("truncated body opens");
                }
            }
            Err(error) => panic!("truncated header opens: {error}"),
        }
        assert!(driver::fs::exists(&path), "truncated bodies stay for retry");
    }

    #[test]
    fn filename_mismatch_reports_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let handle = put_bytes(&store, dir.path(), "named.bin", b"named bytes");
        let home = roots.temp_base.join("blobs");
        let body = match driver::fs::read(&home.join(handle.sha256().hex())) {
            Ok(found) => found,
            Err(error) => panic!("object reads: {error}"),
        };
        let wrong = Sha::hash(b"other bytes");
        let wrong_path = home.join(wrong.hex());
        driver::fs::write(&wrong_path, &body).unwrap();
        let wrong_handle = BlobHandle::new(wrong.clone());
        match store.open(&wrong_handle) {
            Ok(_) => panic!("misnamed blob opens"),
            Err(BlobError::Corrupt { sha: found }) => {
                assert_eq!(found, wrong, "misnamed open keeps the hash")
            }
            Err(error) => panic!("wrong misnamed variant: {error}"),
        }
        assert!(
            driver::fs::exists(&wrong_path),
            "misnamed blobs stay for retry"
        );
        assert!(
            store.resolve(handle.sha256()).is_ok(),
            "the true name still resolves"
        );
    }

    #[test]
    fn receive_refuses_member_and_hash_mismatches() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let handle = put_bytes(&store, dir.path(), "member.bin", b"member bytes");
        let object = roots.temp_base.join("blobs").join(handle.sha256().hex());
        let source = match ResourceHandle::new(dir.path(), object.clone(), handle.sha256().clone())
        {
            Ok(found) => found,
            Err(error) => panic!("member opens: {error}"),
        };
        let wrong = Sha::hash(b"other bytes");
        match store.receive(&wrong, &source) {
            Ok(()) => panic!("misnamed member lands"),
            Err(BlobError::Corrupt { sha: found }) => {
                assert_eq!(found, wrong, "refusal keeps the hash")
            }
            Err(error) => panic!("wrong refusal variant: {error}"),
        }
        assert!(
            driver::fs::exists(&object),
            "refused members stay for retry"
        );
    }

    #[test]
    fn put_shares_files_across_store_instances() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let raw = b"shared file bytes";
        let handle = put_bytes(&store, dir.path(), "shared.bin", raw);
        let (fresh_sender, _) = crossbeam_channel::unbounded();
        let fresh = BlobStore::new(&roots, fresh_sender);
        guard.fail_writes(std::io::ErrorKind::QuotaExceeded);
        match fresh.put(&source_of(dir.path(), "shared.bin", raw)) {
            Ok(again) => assert_eq!(again, handle, "fresh stores reuse the file"),
            Err(error) => panic!("present put skips writes: {error}"),
        }
    }

    #[test]
    fn missing_blob_names_hash() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let handle = BlobHandle::new(Sha::hash(b"absent"));
        assert!(
            store.resolve(handle.sha256()).is_err(),
            "absent ref never resolves"
        );
        match store.open(&handle) {
            Ok(_) => panic!("absent blob opens"),
            Err(error) => assert!(
                error.to_string().contains(&handle.sha256().hex()),
                "missing open shows the hash: {error}"
            ),
        }
        match store.len(&handle) {
            Ok(_) => panic!("absent length reads"),
            Err(error) => assert!(
                error.to_string().contains(&handle.sha256().hex()),
                "missing length shows the hash: {error}"
            ),
        }
    }

    #[test]
    fn file_puts_keep_verified_identity() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store(dir.path());
        let raw = b"shared pool bytes";
        seed_file(dir.path(), "shared.bin", raw);
        let source = source_of(dir.path(), "shared.bin", raw);
        let via_put = match store.put(&source) {
            Ok(handle) => handle,
            Err(error) => panic!("pool stores: {error}"),
        };
        match store.put(&source) {
            Ok(again) => assert_eq!(again, via_put, "repeat put keeps identity"),
            Err(error) => panic!("repeat put stores: {error}"),
        };
        assert_eq!(read_open(&store, &via_put), raw);
        let missing_path = dir.path().join("absent.bin");
        let missing =
            ResourceHandle::new(dir.path(), missing_path.clone(), Sha::hash(b"x")).unwrap();
        match store.put(&missing) {
            Ok(_) => panic!("absent source passes"),
            Err(BlobError::Read { sha, fault }) => {
                assert_eq!(sha, Sha::hash(b"x"), "absent keeps hash");
                assert!(
                    matches!(fault, AccessFault::Missing),
                    "absent source keeps the fault: {fault}"
                );
            }
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    #[test]
    fn prune_keeps_referenced_pool_entries() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        let kept = put_bytes(&store, dir.path(), "kept.bin", b"kept bytes");
        let dropped = put_bytes(&store, dir.path(), "dropped.bin", b"dropped bytes");
        match store.persist(std::slice::from_ref(&kept)) {
            Ok(_) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        let keep = BTreeSet::from([kept.sha256().clone()]);
        match store.prune(&keep) {
            Ok(removed) => assert_eq!(removed, 1, "prune drops the unreferenced"),
            Err(error) => panic!("pool prunes: {error}"),
        }
        assert!(
            store.resolve(kept.sha256()).is_ok(),
            "the referenced blob survives"
        );
        assert!(
            store.resolve(dropped.sha256()).is_err(),
            "the unreferenced dies"
        );
        assert_eq!(read_open(&store, &kept), b"kept bytes");
        match store.prune(&BTreeSet::new()) {
            Ok(removed) => assert_eq!(removed, 1, "an empty keep drains the rest"),
            Err(error) => panic!("pool prunes again: {error}"),
        }
        assert!(
            store.resolve(kept.sha256()).is_err(),
            "resolve refuses the dangling blob"
        );
    }

    #[test]
    fn put_reports_write_storage_full() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let roots = test_roots(dir.path());
        let (sender, _) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&roots, sender);
        seed_file(dir.path(), "full.bin", b"full disk");
        guard.fail_writes(std::io::ErrorKind::StorageFull);
        match store.put(&source_of(dir.path(), "full.bin", b"full disk")) {
            Ok(_) => panic!("full disk passes"),
            Err(BlobError::Write { path, fault }) => {
                assert!(
                    matches!(fault, crate::faults::AccessFault::StorageFull),
                    "put keeps the fault"
                );
                assert!(
                    path.starts_with(roots.temp_base.join("blobs")),
                    "put keeps the cache path"
                );
            }
            Err(error) => panic!("wrong put variant: {error}"),
        }
    }

    #[test]
    fn put_announces_finished_blob_with_counts() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&test_roots(dir.path()), sender);
        let _ = put_bytes(&store, dir.path(), "announced.bin", b"announced bytes");
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::BlobCompressed { done: 1 }],
            "put fires one finished fact"
        );
    }

    #[test]
    fn batch_moves_blob_counters_across_puts() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&test_roots(dir.path()), sender);
        let _ = put_bytes(&store, dir.path(), "first.bin", b"first bytes");
        let _ = put_bytes(&store, dir.path(), "second.bin", b"second bytes");
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                Event::BlobCompressed { done: 1 },
                Event::BlobCompressed { done: 2 },
            ]
        );
    }

    #[test]
    fn reuse_stays_silent() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&test_roots(dir.path()), sender);
        let raw = b"silent reuse bytes";
        let _ = put_bytes(&store, dir.path(), "silent.bin", raw);
        let _ = put_bytes(&store, dir.path(), "silent.bin", raw);
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::BlobCompressed { done: 1 }],
            "reuse fires no fact"
        );
    }

    #[test]
    fn persist_announces_promoting_count() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = BlobStore::new(&test_roots(dir.path()), sender);
        let handle = put_bytes(&store, dir.path(), "promoted.bin", b"promoted bytes");
        let _ = receiver.try_iter().collect::<Vec<_>>();
        match store.persist(std::slice::from_ref(&handle)) {
            Ok(()) => {}
            Err(error) => panic!("persist lands: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::Promoting { blobs: 1 }],
            "persist fires one promoting fact"
        );
    }
}
