//! Content-addressed pool.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use confit_core::error::{
    ContextualCause, ContextualResult, ErrorCause, FilesystemError, WithContext, contextual,
};
use confit_core::sha::{Sha, ShaWriter};
use confit_events::ProgressEvent;
use confit_events::ToUi;
use crossbeam_channel::Sender;
use sha2::Digest as _;

use crate::StoreRoots;
use confit_driver as driver;

/// The folder name for pool entries under the config base.
const BLOBS_DIR: &str = "blobs";

/// The gzip level for pool entries.
const POOL_GZIP_LEVEL: u32 = 6;

/// A store of content-addressed bytes under the config base.
#[derive(Debug, Clone)]
pub struct PoolStore {
    pool: PathBuf,
    to_ui: Sender<ToUi>,
    finished: Arc<AtomicUsize>,
}

impl PoolStore {
    /// Builds a pool over the config base.
    pub fn new(roots: &StoreRoots, to_ui: Sender<ToUi>) -> Self {
        Self {
            pool: roots.config_base.join(BLOBS_DIR),
            to_ui,
            finished: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Returns the file that holds one content hash.
    pub fn path(&self, sha: &Sha) -> PathBuf {
        self.pool.join(sha.hex())
    }

    /// Stores one stream in the pool under its expected hash.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the stream fails the expected hash.
    /// - [`FilesystemError`] when a pool write fails.
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
        let _ = self.to_ui.send(ToUi::Progress(ProgressEvent::Progress {
            id: "pool".to_owned(),
            label: format!("compressing pool entries ({done})"),
            done: u64::try_from(done).unwrap_or(u64::MAX),
            total: None,
        }));
        Ok(())
    }

    /// Opens one pool entry as a stream that verifies its hash.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the pool entry is absent or unreadable.
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

    /// Removes pool files that the keep set leaves out.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when listing or removal fails.
    pub fn prune(&self, keep: &BTreeSet<Sha>) -> ContextualResult<usize> {
        prune_dir(&self.pool, keep)
    }
}

/// A reader that checks each body against its expected hash.
pub struct VerifiedRead<R> {
    inner: R,
    hasher: sha2::Sha256,
    expected: Sha,
}

impl<R: std::io::Read> VerifiedRead<R> {
    /// Builds a verified reader over one body stream.
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

/// Removes the files in one pool folder that the keep set leaves out.
fn prune_dir(dir: &Path, keep: &BTreeSet<Sha>) -> ContextualResult<usize> {
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

/// Returns true if the pool file is in the keep set.
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

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        }
    }

    fn test_store(dir: &Path) -> PoolStore {
        let (sender, _) = crossbeam_channel::unbounded();
        PoolStore::new(&test_roots(dir), sender)
    }

    /// A test reader that fails every read.
    struct FailRead;

    impl std::io::Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("the stream is not read"))
        }
    }

    fn read_open(store: &PoolStore, sha: &Sha) -> Vec<u8> {
        match store.open(sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                found
            }
            Err(error) => panic!("the pooled bytes fail to open: {error}"),
        }
    }

    #[test]
    fn pool_open_rejects_tampered_body() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"original bytes");
        match store.store_checked(&sha, &b"original bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
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
                    Ok(_) => panic!("expected a tampered body error"),
                    Err(error) => {
                        let mismatch = error
                            .get_ref()
                            .and_then(|cause| cause.downcast_ref::<ErrorCause>())
                            .unwrap_or_else(|| panic!("expected a content hash mismatch: {error}"));
                        match mismatch {
                            ErrorCause::ContentHashMismatch { expected, found } => {
                                assert_eq!(expected, &sha, "the mismatch keeps the expected hash");
                                assert_eq!(
                                    found,
                                    &Sha::hash(b"tampered bytes"),
                                    "the mismatch keeps the found hash"
                                );
                            }
                            other => {
                                panic!("expected a content hash mismatch: {other}")
                            }
                        }
                    }
                }
            }
            Err(error) => panic!("the tampered body fails to open: {error}"),
        }
    }

    #[test]
    fn pool_store_and_open_round_trip_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw = b"pooled bytes";
        let sha = Sha::hash(raw);
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        assert_eq!(
            read_open(&store, &sha),
            raw,
            "the pool opens the stored bytes"
        );
    }

    #[test]
    fn pool_store_announces_a_finished_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = PoolStore::new(&test_roots(dir.path()), sender);
        let raw = b"announced bytes";
        match store.store_checked(&Sha::hash(raw), &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![ToUi::Progress(ProgressEvent::Progress {
                id: "pool".to_owned(),
                label: "compressing pool entries (1)".to_owned(),
                done: 1,
                total: None,
            })],
            "the pool store writes one finished event"
        );
    }

    #[test]
    fn pool_store_stays_silent_on_repeat() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = PoolStore::new(&test_roots(dir.path()), sender);
        let raw = b"silent reuse bytes";
        let sha = Sha::hash(raw);
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("the repeated store fails to store: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![ToUi::Progress(ProgressEvent::Progress {
                id: "pool".to_owned(),
                label: "compressing pool entries (1)".to_owned(),
                done: 1,
                total: None,
            })],
            "the repeated store writes no event"
        );
    }

    #[test]
    fn pool_store_skips_the_stream_for_a_present_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, receiver) = crossbeam_channel::unbounded();
        let store = PoolStore::new(&test_roots(dir.path()), sender);
        let raw = b"present bytes";
        let sha = Sha::hash(raw);
        match store.store_checked(&sha, &raw[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        match store.store_checked(&sha, FailRead) {
            Ok(()) => {}
            Err(error) => panic!("the present store fails to store: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![ToUi::Progress(ProgressEvent::Progress {
                id: "pool".to_owned(),
                label: "compressing pool entries (1)".to_owned(),
                done: 1,
                total: None,
            })],
            "the present store writes no event"
        );
    }

    #[test]
    fn pool_open_names_the_pool_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"absent");
        match store.open(&sha) {
            Ok(_) => panic!("expected an absent content error"),
            Err(error) => match &error.context.cache_path {
                Some(found) => {
                    assert_eq!(found, &store.path(&sha), "the miss keeps the cache path")
                }
                None => panic!("expected the cache path: {error}"),
            },
        }
    }

    #[test]
    fn pool_store_rejects_a_mismatched_stream() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        let other_sha = Sha::hash(b"garbage");
        match store.store_checked(&sha, &b"garbage"[..]) {
            Ok(()) => panic!("expected a mismatched stream error"),
            Err(error) => {
                match error.cause() {
                    ErrorCause::ContentHashMismatch { expected, found } => {
                        assert_eq!(expected, &sha, "the mismatch keeps the expected hash");
                        assert_eq!(found, &other_sha, "the mismatch keeps the found hash");
                    }
                    other => panic!("expected a content hash mismatch: {other}"),
                }
                match &error.context.cache_path {
                    Some(found) => assert_eq!(
                        found,
                        &store.path(&sha),
                        "the mismatch keeps the cache path"
                    ),
                    None => panic!("expected the cache path: {error}"),
                }
            }
        }
        assert!(!store.path(&sha).exists(), "the mismatch leaves no file");
    }

    #[test]
    fn pool_prune_keeps_referenced_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let kept = Sha::hash(b"kept bytes");
        let dropped = Sha::hash(b"dropped bytes");
        match store.store_checked(&kept, &b"kept bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        match store.store_checked(&dropped, &b"dropped bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        let keep = BTreeSet::from([kept.clone()]);
        match store.prune(&keep) {
            Ok(removed) => assert_eq!(removed, 1, "the prune drops the unreferenced entry"),
            Err(error) => panic!("the pool fails to prune: {error}"),
        }
        assert_eq!(read_open(&store, &kept), b"kept bytes");
        assert!(
            store.open(&dropped).is_err(),
            "the pool holds no pruned entry"
        );
    }
}
