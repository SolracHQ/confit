//! Content-addressed pool.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use confit_core::error::{
    ContextualCause, ContextualResult, ErrorCause, FilesystemError, WithContext, contextual,
};
use confit_core::progress::{Event, ProgressSender};
use confit_core::sha::{Sha, ShaWriter};
use sha2::Digest as _;

use crate::StoreRoots;
use confit_driver as driver;

/// Pool folder name under the config base.
const BLOBS_DIR: &str = "blobs";

/// Pool gzip level.
const POOL_GZIP_LEVEL: u32 = 6;

/// Content-addressed pool.
#[derive(Debug, Clone)]
pub struct PoolStore {
    pool: PathBuf,
    progress: ProgressSender,
    finished: Arc<AtomicUsize>,
}

impl PoolStore {
    /// Build one file-backed pool.
    pub fn new(roots: &StoreRoots, progress: ProgressSender) -> Self {
        Self {
            pool: roots.config_base.join(BLOBS_DIR),
            progress,
            finished: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Derive the pool file for one content hash.
    pub fn path(&self, sha: &Sha) -> PathBuf {
        self.pool.join(sha.hex())
    }

    /// Compress one stream into the pool under the expected hash.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for streams failing the expected hash.
    /// - [`FilesystemError`] for pool write faults.
    pub fn store_checked(&self, sha: &Sha, mut reader: impl std::io::Read) -> ContextualResult<()> {
        let dest = self.path(sha);
        if dest.exists() {
            return Ok(());
        }
        let raw = driver::StagedFile::stage(&dest).with_cache_path(&dest)?;
        let found = {
            let sink = std::fs::File::create(raw.path())
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            let mut writer = ShaWriter::new(sink);
            std::io::copy(&mut reader, &mut writer)
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            writer.digest()
        };
        if found != *sha {
            return Err(contextual(ErrorCause::ContentHashMismatch {
                expected: sha.clone(),
                found,
            })
            .with_cache_path(dest));
        }
        let staged = driver::StagedFile::stage(&dest).with_cache_path(&dest)?;
        {
            let sink = std::fs::File::create(staged.path())
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            let mut encoder =
                flate2::write::GzEncoder::new(sink, flate2::Compression::new(POOL_GZIP_LEVEL));
            let mut source = std::fs::File::open(raw.path())
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            std::io::copy(&mut source, &mut encoder)
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            encoder
                .finish()
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
        }
        staged.persist(&dest).with_cache_path(&dest)?;
        let done = self.finished.fetch_add(1, Ordering::Relaxed) + 1;
        let _ = self.progress.send(Event::PoolStored { done });
        Ok(())
    }

    /// Open a verified byte stream for one content hash.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for missing and unreadable pool entries.
    pub fn open(
        &self,
        sha: &Sha,
    ) -> ContextualResult<VerifiedRead<flate2::read::GzDecoder<std::fs::File>>> {
        let path = self.path(sha);
        let file = std::fs::File::open(&path)
            .map_err(ContextualCause::from)
            .with_cache_path(&path)?;
        Ok(VerifiedRead::new(
            flate2::read::GzDecoder::new(file),
            sha.clone(),
        ))
    }

    /// Drop pool files outside the keep set.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for listing and removal failures.
    pub fn prune(&self, keep: &BTreeSet<Sha>) -> ContextualResult<usize> {
        prune_dir(&self.pool, keep)
    }
}

/// Pool body reader verifying its content hash.
pub struct VerifiedRead<R> {
    inner: R,
    hasher: sha2::Sha256,
    expected: Sha,
}

impl<R: std::io::Read> VerifiedRead<R> {
    /// Wrap one body stream with its expected digest.
    pub fn new(inner: R, expected: Sha) -> Self {
        Self {
            inner,
            hasher: sha2::Sha256::new(),
            expected,
        }
    }
}

impl<R: std::io::Read> std::io::Read for VerifiedRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        if read == 0 {
            let found = Sha::finish(self.hasher.clone());
            if found != self.expected {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    ErrorCause::ContentHashMismatch {
                        expected: self.expected.clone(),
                        found,
                    },
                ));
            }
            return Ok(0);
        }
        self.hasher.update(&buf[..read]);
        Ok(read)
    }
}

/// Drop files in one pool home outside the keep set.
fn prune_dir(dir: &std::path::Path, keep: &BTreeSet<Sha>) -> ContextualResult<usize> {
    let entries = match driver::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error)
            if matches!(
                error.cause(),
                ErrorCause::FilesystemError(FilesystemError::FileMissing)
            ) =>
        {
            return Ok(0);
        }
        Err(error) => {
            return Err(error);
        }
    };
    let mut removed = 0;
    for path in entries {
        if held(&path, keep) {
            continue;
        }
        std::fs::remove_file(&path)
            .map_err(ContextualCause::from)
            .with_cache_path(&path)?;
        removed += 1;
    }
    Ok(removed)
}

/// Report whether one pool home file is inside the keep set.
fn held(path: &std::path::Path, keep: &BTreeSet<Sha>) -> bool {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .and_then(|name| Sha::new(name).ok())
        .is_some_and(|content| keep.contains(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Read as _;

    fn test_roots(dir: &std::path::Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        }
    }

    fn test_store(dir: &std::path::Path) -> PoolStore {
        let (sender, _) = crossbeam_channel::unbounded();
        PoolStore::new(&test_roots(dir), sender)
    }

    /// Reader failing on every read.
    struct FailRead;

    impl std::io::Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("stream stays unread"))
        }
    }

    fn read_open(store: &PoolStore, sha: &Sha) -> Vec<u8> {
        match store.open(sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                found
            }
            Err(error) => panic!("pooled bytes open: {error}"),
        }
    }

    #[test]
    fn open_rejects_tampered_body() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"original bytes");
        match store.store_checked(&sha, &b"original bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        let dest = store.path(&sha);
        let sink = std::fs::File::create(&dest).unwrap();
        let mut body = flate2::write::GzEncoder::new(sink, flate2::Compression::default());
        body.write_all(b"tampered bytes").unwrap();
        body.finish().unwrap();
        match store.open(&sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                match reader.read_to_end(&mut found) {
                    Ok(_) => panic!("tampered body opens"),
                    Err(error) => {
                        let mismatch = error
                            .get_ref()
                            .and_then(|cause| cause.downcast_ref::<ErrorCause>())
                            .unwrap_or_else(|| panic!("tampered keeps mismatch: {error}"));
                        match mismatch {
                            ErrorCause::ContentHashMismatch { expected, found } => {
                                assert_eq!(expected, &sha, "mismatch keeps expected");
                                assert_eq!(
                                    found,
                                    &Sha::hash(b"tampered bytes"),
                                    "mismatch keeps found"
                                );
                            }
                            other => panic!("wrong tamper cause: {other}"),
                        }
                    }
                }
            }
            Err(error) => panic!("tampered open fails: {error}"),
        }
    }

    #[test]
    fn store_open_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw = b"pooled bytes";
        let sha = Sha::hash(raw);
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        assert_eq!(read_open(&store, &sha), raw, "open round-trips bytes");
    }

    #[test]
    fn store_announces_finished_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = PoolStore::new(&test_roots(dir.path()), sender);
        let raw = b"announced bytes";
        match store.store_checked(&Sha::hash(raw), &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::PoolStored { done: 1 }],
            "store fires one finished fact"
        );
    }

    #[test]
    fn repeat_store_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = PoolStore::new(&test_roots(dir.path()), sender);
        let raw = b"silent reuse bytes";
        let sha = Sha::hash(raw);
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("repeat store skips: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::PoolStored { done: 1 }],
            "repeat store fires no fact"
        );
    }

    #[test]
    fn present_store_never_parsed_the_stream() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = PoolStore::new(&test_roots(dir.path()), sender);
        let raw = b"present bytes";
        let sha = Sha::hash(raw);
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        match store.store_checked(&sha, FailRead) {
            Ok(()) => {}
            Err(error) => panic!("present store skips the stream: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![Event::PoolStored { done: 1 }],
            "present store fires no fact"
        );
    }

    #[test]
    fn missing_open_names_pool_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"absent");
        match store.open(&sha) {
            Ok(_) => panic!("absent content opens"),
            Err(error) => match &error.context.cache_path {
                Some(found) => assert_eq!(found, &store.path(&sha), "miss keeps the cache path"),
                None => panic!("miss keeps the cache path: {error}"),
            },
        }
    }

    #[test]
    fn reject_mismatched_stream() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        let wrong = Sha::hash(b"garbage");
        match store.store_checked(&sha, &b"garbage"[..]) {
            Ok(()) => panic!("mismatched stream passes"),
            Err(error) => {
                match error.cause() {
                    confit_core::error::ErrorCause::ContentHashMismatch { expected, found } => {
                        assert_eq!(expected, &sha, "mismatch keeps expected");
                        assert_eq!(found, &wrong, "mismatch keeps found");
                    }
                    other => panic!("wrong mismatch cause: {other}"),
                }
                match &error.context.cache_path {
                    Some(found) => assert_eq!(found, &store.path(&sha), "mismatch keeps the path"),
                    None => panic!("mismatch keeps the path: {error}"),
                }
            }
        }
        assert!(!store.path(&sha).exists(), "mismatch leaves no file");
    }

    #[test]
    fn prune_keeps_referenced_pool_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let kept = Sha::hash(b"kept bytes");
        let dropped = Sha::hash(b"dropped bytes");
        match store.store_checked(&kept, &b"kept bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        match store.store_checked(&dropped, &b"dropped bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        let keep = BTreeSet::from([kept.clone()]);
        match store.prune(&keep) {
            Ok(removed) => assert_eq!(removed, 1, "prune drops the unreferenced"),
            Err(error) => panic!("pool prunes: {error}"),
        }
        assert_eq!(read_open(&store, &kept), b"kept bytes");
        assert!(store.open(&dropped).is_err(), "the unreferenced dies");
    }
}
