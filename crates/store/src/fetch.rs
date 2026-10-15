//! Content-addressed fetch cache for remote bytes.

pub mod error;
pub mod transport;

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use confit_driver::progress::{Event, ProgressSender};
use confit_driver::sha::Sha;
use sha2::Digest as _;

use self::transport::BODY_LIMIT_BYTES;
use crate::StoreRoots;
use crate::handles::ResourceHandle;
use confit_driver as driver;
use error::{FetchError, Result};

/// Chunk size for streamed cache writes.
const STREAM_BUF_BYTES: usize = 8 * 1024;

/// Progress step for in-flight download events.
const PROGRESS_STEP_BYTES: u64 = 64 * 1024;

/// The url-hash to content-hash index folder.
const INDEX_DIR: &str = "urls";

/// File-backed fetch cache for remote bytes.
#[derive(Clone, Debug)]
pub struct FetchCache {
    cache: PathBuf,
    progress: ProgressSender,
}

impl FetchCache {
    /// Build one file-backed fetch cache under the temp base.
    pub fn new(roots: &StoreRoots, progress: ProgressSender) -> Self {
        Self {
            cache: roots.temp_base.clone(),
            progress,
        }
    }

    /// Derive the content path for one content digest.
    fn content_path(&self, sha_hex: &str) -> PathBuf {
        self.cache.join(sha_hex)
    }

    /// Derive the index entry path for one URL.
    fn index_path(&self, url: &str) -> PathBuf {
        self.cache
            .join(INDEX_DIR)
            .join(Sha::hash(url.as_bytes()).hex())
    }

    /// Derive the pending path for one URL download.
    fn pending_path(&self, url: &str) -> PathBuf {
        driver::pending_path(&self.content_path(&Sha::hash(url.as_bytes()).hex()))
    }

    /// Find a cached handle with a valid index digest.
    ///
    /// - Tampered entries are misses.
    fn lookup(&self, url: &str) -> Option<ResourceHandle> {
        let stored = driver::fs::read_to_string(&self.index_path(url)).ok()?;
        let wanted = Sha::new(stored.trim().to_lowercase()).ok()?;
        let content = self.content_path(&wanted.hex());
        let mut file = driver::fs::open(&content).ok()?;
        let actual = Sha::read(&mut file).ok()?;
        if actual != wanted {
            return None;
        }
        ResourceHandle::new(&self.cache, content, actual).ok()
    }

    /// Serve one cached hit with the cached progress event.
    ///
    /// # Errors
    ///
    /// - [`FetchError::DigestMismatch`] for cached hits failing the user sha.
    fn fetch_hit(&self, url: &str, expected_sha: Option<&Sha>) -> Option<Result<ResourceHandle>> {
        let hit = self.lookup(url)?;
        let _ = self.progress.send(Event::FetchCached {
            url: url.to_string(),
            bytes: file_len(hit.canonical()),
        });
        match check_sha(url, hit.sha256(), expected_sha) {
            Ok(()) => Some(Ok(hit)),
            Err(error) => Some(Err(error)),
        }
    }

    /// Download one URL body with the downloaded progress event.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Status`] for refused statuses.
    /// - [`FetchError::Timeout`] for timeouts.
    /// - [`FetchError::Read`] for failed cache reads.
    /// - [`FetchError::Write`] for cache write faults.
    /// - [`FetchError::WriteUnknown`] for other cache write failures.
    /// - [`FetchError::DigestMismatch`] for sha mismatches.
    fn fetch_download(&self, url: &str, expected_sha: Option<&Sha>) -> Result<ResourceHandle> {
        let (path, sha, bytes) = self.download(url)?;
        let _ = self.progress.send(Event::FetchDownloaded {
            url: url.to_string(),
            bytes,
        });
        check_sha(url, &sha, expected_sha)?;
        ResourceHandle::new(&self.cache, path, sha).map_err(|error| FetchError::Read {
            url: url.to_owned(),
            fault: crate::faults::AccessFault::Unknown {
                message: error.to_string(),
            },
        })
    }

    /// Download one URL body into the cache through pending.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Status`] for refused statuses.
    /// - [`FetchError::Timeout`] for timeouts.
    /// - [`FetchError::Read`] for failed cache reads.
    /// - [`FetchError::Write`] for cache write faults.
    /// - [`FetchError::WriteUnknown`] for other cache write failures.
    /// - [`FetchError::Unscripted`] for unscripted test urls.
    fn download(&self, url: &str) -> Result<(PathBuf, Sha, usize)> {
        let (reader, total) = transport::download(url, BODY_LIMIT_BYTES)?;
        self.store_stream(url, reader, total)
    }

    /// Write a streamed body with the index entry for one URL.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Read`] for failed cache reads.
    /// - [`FetchError::Write`] for cache write faults.
    /// - [`FetchError::WriteUnknown`] for other cache write failures.
    fn store_stream(
        &self,
        url: &str,
        reader: impl std::io::Read,
        total: Option<u64>,
    ) -> Result<(PathBuf, Sha, usize)> {
        let pending = self.pending_path(url);
        driver::fs::create_parent(&pending)
            .map_err(|error| FetchError::from_write_io(url, error))?;
        let outcome = self.stream_to_pending(url, reader, &pending, total);
        let (digest, bytes) = match outcome {
            Ok(done) => done,
            Err(error) => {
                let _ = driver::fs::remove_file(&pending);
                return Err(error);
            }
        };
        let content = self.content_path(&digest.hex());
        driver::atomic_write(&content, |final_pending| {
            driver::fs::rename(&pending, final_pending)
        })
        .map_err(|error| FetchError::from_write_io(url, error))?;
        self.write_index(url, &digest)?;
        Ok((content, digest, bytes))
    }

    /// Write the url index entry for one content digest.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Write`] for cache write faults.
    /// - [`FetchError::WriteUnknown`] for other cache write failures.
    fn write_index(&self, url: &str, digest: &Sha) -> Result<()> {
        let index = self.index_path(url);
        driver::fs::create_parent(&index).map_err(|error| FetchError::from_write_io(url, error))?;
        driver::fs::write(&index, digest.hex().as_bytes())
            .map_err(|error| FetchError::from_write_io(url, error))
    }

    /// Stream one body into pending with a hash.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Read`] for failed pending reads.
    /// - [`FetchError::Write`] for cache write faults.
    /// - [`FetchError::WriteUnknown`] for other cache write failures.
    fn stream_to_pending(
        &self,
        url: &str,
        reader: impl std::io::Read,
        pending: &Path,
        total: Option<u64>,
    ) -> Result<(Sha, usize)> {
        let file =
            driver::fs::create(pending).map_err(|error| FetchError::from_write_io(url, error))?;
        let mut writer = std::io::BufWriter::new(file);
        let mut limited = reader.take(BODY_LIMIT_BYTES);
        let mut hasher = sha2::Sha256::new();
        let mut buf = [0u8; STREAM_BUF_BYTES];
        let mut bytes: u64 = 0;
        let mut reported: u64 = 0;
        loop {
            let read = limited
                .read(&mut buf)
                .map_err(|error| FetchError::from_io(url, error))?;
            if read == 0 {
                break;
            }
            bytes += read as u64;
            hasher.update(&buf[..read]);
            writer
                .write_all(&buf[..read])
                .map_err(|error| FetchError::from_write_io(url, error))?;
            if bytes - reported >= PROGRESS_STEP_BYTES {
                reported = bytes;
                let _ = self.progress.send(Event::FetchProgress {
                    url: url.to_string(),
                    bytes_so_far: bytes,
                    total,
                });
            }
        }
        writer
            .flush()
            .map_err(|error| FetchError::from_write_io(url, error))?;
        let digest = Sha::finish(hasher);
        Ok((digest, usize::try_from(bytes).unwrap_or(0)))
    }
}

impl FetchCache {
    /// Fetch one URL into a resource handle.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Status`] for refused statuses.
    /// - [`FetchError::Timeout`] for timeouts.
    /// - [`FetchError::Read`] for failed cache reads.
    /// - [`FetchError::Write`] for cache write faults.
    /// - [`FetchError::WriteUnknown`] for other cache write failures.
    /// - [`FetchError::Unscripted`] for unscripted test urls.
    /// - [`FetchError::DigestMismatch`] for sha mismatches.
    pub fn fetch(
        &self,
        url: &str,
        expected_sha: Option<Sha>,
        re_fetch: bool,
    ) -> Result<ResourceHandle> {
        let _ = self.progress.send(Event::FetchStarted {
            url: url.to_string(),
        });
        if !re_fetch && let Some(hit) = self.fetch_hit(url, expected_sha.as_ref()) {
            return hit;
        }
        self.fetch_download(url, expected_sha.as_ref())
    }

    /// Read cached bytes for one resource handle.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Read`] for failed cache reads.
    pub fn read(&self, handle: &ResourceHandle, url: &str) -> Result<Vec<u8>> {
        driver::fs::read(handle.canonical()).map_err(|error| FetchError::from_io(url, error))
    }

    /// Open cached bytes for one resource handle.
    ///
    /// # Errors
    ///
    /// - [`FetchError::Read`] for failed cache reads.
    pub fn open(&self, handle: &ResourceHandle, url: &str) -> Result<Box<dyn std::io::Read>> {
        driver::fs::open(handle.canonical())
            .map(|file| file as Box<dyn std::io::Read>)
            .map_err(|error| FetchError::from_io(url, error))
    }
}

/// Read one file length.
///
/// - Missing files are zero.
fn file_len(path: &Path) -> usize {
    match driver::fs::metadata(path) {
        Ok(facts) => usize::try_from(facts.len()).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Check a fetched digest against the user sha.
///
/// # Errors
///
/// - [`FetchError::DigestMismatch`] for mismatches.
fn check_sha(url: &str, actual: &Sha, expected: Option<&Sha>) -> Result<()> {
    let Some(wanted) = expected else {
        return Ok(());
    };
    if actual != wanted {
        return Err(FetchError::DigestMismatch {
            url: url.to_owned(),
            want: wanted.clone(),
            got: actual.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::{self as driver, fs::TestGuard};

    const URL: &str = "https://example.com/tool.bin";
    const FILE_URL: &str = "https://example.com/tool.tar.gz";

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            temp_base: dir.join("temp"),
            ..Default::default()
        }
    }

    fn file_cache(dir: &Path) -> FetchCache {
        let (sender, _) = crossbeam_channel::unbounded();
        FetchCache::new(&test_roots(dir), sender)
    }

    fn index_file(dir: &Path, url: &str) -> PathBuf {
        dir.join("temp")
            .join(INDEX_DIR)
            .join(Sha::hash(url.as_bytes()).hex())
    }

    fn content_file(dir: &Path, sha_hex: &str) -> PathBuf {
        dir.join("temp").join(sha_hex)
    }

    fn seed(cache: &FetchCache, url: &str, body: &[u8]) -> ResourceHandle {
        driver::http::serve(url, body);
        match cache.fetch(url, None, false) {
            Ok(handle) => handle,
            Err(error) => panic!("seeded body serves: {error}"),
        }
    }

    #[test]
    fn bytes_hit_serves_seeded_entry() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        assert_eq!(driver::http::calls(URL), 1, "seeding downloads once");
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let found = driver::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
                assert_eq!(driver::http::calls(URL), 1, "hit makes no download");
            }
            Err(error) => panic!("seeded bytes serve: {error}"),
        }
    }

    #[test]
    fn bytes_hit_makes_no_download() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let found = driver::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
                assert_eq!(driver::http::calls(URL), 1, "offline hit calls no download");
            }
            Err(error) => panic!("offline hit serves: {error}"),
        }
    }

    #[test]
    fn file_hit_returns_path_under_cache() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, FILE_URL, b"binary");
        match cache.fetch(FILE_URL, None, false) {
            Ok(handle) => {
                let path = handle.canonical().to_path_buf();
                assert!(
                    path.starts_with(dir.path().join("temp")),
                    "cache path stays under temp: {}",
                    path.display()
                );
                match driver::fs::read(&path) {
                    Ok(found) => assert_eq!(found, b"binary".to_vec()),
                    Err(error) => panic!("cached file reads: {error}"),
                }
            }
            Err(error) => panic!("cached file serves: {error}"),
        }
    }

    #[test]
    fn tamper_reads_as_miss_without_serving() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        let handle = seed(&cache, URL, b"genuine");
        assert!(cache.lookup(URL).is_some());
        driver::fs::write(handle.canonical(), b"tampered").unwrap();
        assert!(cache.lookup(URL).is_none());
        match cache.fetch(URL, None, false) {
            Ok(refetched) => {
                let found = driver::fs::read(refetched.canonical()).unwrap();
                assert_eq!(found, b"genuine".to_vec(), "tamper redownloads");
                assert_ne!(found, b"tampered".to_vec(), "tampered bytes never serve");
                assert_eq!(driver::http::calls(URL), 2, "tamper redownloads once");
            }
            Err(error) => panic!("tampered entry redownloads: {error}"),
        }
    }

    #[test]
    fn missing_index_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        driver::fs::remove_file(&index_file(dir.path(), URL)).unwrap();
        assert!(cache.lookup(URL).is_none());
        match cache.fetch(URL, None, false) {
            Ok(refetched) => {
                let found = driver::fs::read(refetched.canonical()).unwrap();
                assert_eq!(found, b"genuine".to_vec(), "index loss redownloads");
                assert_eq!(driver::http::calls(URL), 2, "index loss redownloads once");
            }
            Err(error) => panic!("index-less entry redownloads: {error}"),
        }
    }

    #[test]
    fn corrupt_index_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        assert!(cache.lookup(URL).is_some());
        driver::fs::write(&index_file(dir.path(), URL), b"not-a-hex-digest").unwrap();
        assert!(cache.lookup(URL).is_none());
        match cache.fetch(URL, None, false) {
            Ok(refetched) => {
                let found = driver::fs::read(refetched.canonical()).unwrap();
                assert_eq!(found, b"genuine".to_vec(), "corrupt index redownloads");
            }
            Err(error) => panic!("corrupt index redownloads: {error}"),
        }
    }

    #[test]
    fn layout_uses_content_address_with_url_index() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        assert_eq!(
            content_file(dir.path(), &wanted.hex()),
            dir.path().join("temp").join(wanted.hex())
        );
        assert!(driver::fs::exists(&content_file(dir.path(), &wanted.hex())));
        let index = index_file(dir.path(), URL);
        assert_eq!(
            index,
            dir.path()
                .join("temp")
                .join(INDEX_DIR)
                .join(Sha::hash(URL.as_bytes()).hex())
        );
        assert_eq!(driver::fs::read_to_string(&index).unwrap(), wanted.hex());
        assert_no_sha_suffix_files(&dir.path().join("temp"));
    }

    #[test]
    fn hit_reports_started_and_cached_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let (sender, receiver) = crossbeam_channel::unbounded();
        let cache = FetchCache::new(&test_roots(dir.path()), sender);
        match cache.fetch(URL, None, false) {
            Ok(_) => {
                let events: Vec<Event> = receiver.try_iter().collect();
                assert_eq!(
                    events,
                    vec![
                        Event::FetchStarted {
                            url: URL.to_string(),
                        },
                        Event::FetchCached {
                            url: URL.to_string(),
                            bytes: 5,
                        },
                    ]
                );
                assert_eq!(driver::http::calls(URL), 1, "hit makes no download");
            }
            Err(error) => panic!("hit reports progress: {error}"),
        }
    }

    #[test]
    fn re_fetch_reports_started_and_downloaded_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let (sender, receiver) = crossbeam_channel::unbounded();
        let cache = FetchCache::new(&test_roots(dir.path()), sender);
        match cache.fetch(URL, None, true) {
            Ok(_) => {
                let events: Vec<Event> = receiver.try_iter().collect();
                assert_eq!(
                    events,
                    vec![
                        Event::FetchStarted {
                            url: URL.to_string(),
                        },
                        Event::FetchDownloaded {
                            url: URL.to_string(),
                            bytes: 5,
                        },
                    ]
                );
                assert_eq!(driver::http::calls(URL), 2, "re-fetch downloads once");
            }
            Err(error) => panic!("re-fetch reports progress: {error}"),
        }
    }

    #[test]
    fn batch_reports_downloads_then_hit() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        driver::http::serve(URL, b"1.2.3");
        driver::http::serve(FILE_URL, b"binary");
        let (sender, receiver) = crossbeam_channel::unbounded();
        let cache = FetchCache::new(&test_roots(dir.path()), sender);
        match cache.fetch(URL, None, false) {
            Ok(_) => {}
            Err(error) => panic!("first miss downloads: {error}"),
        }
        match cache.fetch(FILE_URL, None, false) {
            Ok(_) => {}
            Err(error) => panic!("second miss downloads: {error}"),
        }
        match cache.fetch(URL, None, false) {
            Ok(_) => {}
            Err(error) => panic!("repeat hit serves: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                Event::FetchStarted {
                    url: URL.to_string(),
                },
                Event::FetchDownloaded {
                    url: URL.to_string(),
                    bytes: 5,
                },
                Event::FetchStarted {
                    url: FILE_URL.to_string(),
                },
                Event::FetchDownloaded {
                    url: FILE_URL.to_string(),
                    bytes: 6,
                },
                Event::FetchStarted {
                    url: URL.to_string(),
                },
                Event::FetchCached {
                    url: URL.to_string(),
                    bytes: 5,
                },
            ]
        );
    }

    fn assert_no_sha_suffix_files(dir: &Path) {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for path in driver::fs::read_dir(&next).unwrap() {
                if driver::fs::metadata(&path).unwrap().is_dir() {
                    stack.push(path);
                } else {
                    assert!(
                        path.extension().is_none_or(|ext| ext != "sha"),
                        "unexpected sha-suffixed file remains: {}",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn user_sha_mismatch_fails_after_hit() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        seed(&cache, FILE_URL, b"binary");
        let wrong = "0".repeat(64);
        match cache.fetch(URL, Some(Sha::new(wrong.clone()).unwrap()), false) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(URL), "error shows the url: {text}");
                assert!(
                    text.contains("sha256 mismatch"),
                    "error reports loss: {text}"
                );
            }
        }
        match cache.fetch(FILE_URL, Some(Sha::new(wrong.clone()).unwrap()), false) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(FILE_URL), "error shows the url: {text}");
                assert!(
                    text.contains("sha256 mismatch"),
                    "error reports loss: {text}"
                );
            }
        }
        assert_eq!(
            driver::http::calls(URL),
            1,
            "user-sha failure downloads nothing"
        );
    }

    #[test]
    fn user_sha_match_passes_after_hit() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        match cache.fetch(URL, Some(wanted.clone()), false) {
            Ok(handle) => {
                let found = driver::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("good user sha serves: {error}"),
        }
    }

    #[test]
    fn re_fetch_forces_download_past_valid_entry() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let found = driver::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
                assert_eq!(driver::http::calls(URL), 1, "hit makes no download");
            }
            Err(error) => panic!("cached read serves: {error}"),
        }
        match cache.fetch(URL, None, true) {
            Ok(refetched) => {
                let found = driver::fs::read(refetched.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec(), "forced fetch redownloads");
                assert_eq!(driver::http::calls(URL), 2, "forced fetch downloads once");
            }
            Err(error) => panic!("forced fetch downloads: {error}"),
        }
    }

    #[test]
    fn bytes_hit_streams_large_entry() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > STREAM_BUF_BYTES);
        seed(&cache, URL, &raw);
        assert!(cache.lookup(URL).is_some());
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                assert_eq!(handle.sha256(), &Sha::hash(&raw));
                let found = driver::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("large hit serves: {error}"),
        }
    }

    #[test]
    fn open_streams_seeded_body() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        match cache.fetch(URL, None, false) {
            Ok(handle) => match cache.open(&handle, URL) {
                Ok(mut reader) => {
                    let mut found = Vec::new();
                    reader.read_to_end(&mut found).unwrap();
                    assert_eq!(found, b"1.2.3".to_vec());
                }
                Err(error) => panic!("seeded body opens: {error}"),
            },
            Err(error) => panic!("seeded body serves: {error}"),
        }
    }

    #[test]
    fn open_streams_large_body() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > STREAM_BUF_BYTES);
        seed(&cache, URL, &raw);
        match cache.fetch(URL, None, false) {
            Ok(handle) => match cache.open(&handle, URL) {
                Ok(mut reader) => {
                    let mut found = Vec::new();
                    reader.read_to_end(&mut found).unwrap();
                    assert_eq!(found, raw);
                }
                Err(error) => panic!("large body opens: {error}"),
            },
            Err(error) => panic!("large body serves: {error}"),
        }
    }

    #[test]
    fn open_missing_names_cache_path() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        let missing = dir.path().join("cache").join("missing-body");
        let handle = ResourceHandle::new(
            &dir.path().join("cache"),
            missing.clone(),
            Sha::hash(b"absent"),
        )
        .unwrap();
        match cache.open(&handle, URL) {
            Ok(_) => panic!("missing body opens"),
            Err(FetchError::Read { url, fault }) => {
                use crate::faults::AccessFault;
                assert_eq!(url, URL, "missing open keeps the origin url");
                assert!(
                    matches!(fault, AccessFault::Missing),
                    "missing open keeps the fault: {fault}"
                );
            }
            Err(error) => panic!("wrong missing variant: {error}"),
        }
    }

    #[test]
    fn read_serves_whole_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"whole view bytes");
        match cache.fetch(URL, None, false) {
            Ok(handle) => match cache.read(&handle, URL) {
                Ok(found) => assert_eq!(found, b"whole view bytes".to_vec()),
                Err(error) => panic!("seeded body reads: {error}"),
            },
            Err(error) => panic!("seeded body serves: {error}"),
        }
    }

    #[test]
    fn scripted_failure_reads_as_download_error_showing_url() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        driver::http::fail(URL, "boom");
        match cache.fetch(URL, None, false) {
            Ok(_) => panic!("scripted failure serves"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("cannot fetch"),
                    "failure is download error: {text}"
                );
                assert!(text.contains(URL), "error shows the url: {text}");
                assert!(text.contains("boom"), "error keeps the cause: {text}");
            }
        }
        assert_eq!(driver::http::calls(URL), 1, "one download attempt per miss");
        match cache.fetch(URL, None, false) {
            Ok(_) => panic!("scripted failure serves"),
            Err(error) => assert!(
                error.to_string().contains(URL),
                "retry shows the url: {error}"
            ),
        }
        assert_eq!(driver::http::calls(URL), 2, "retry downloads once more");
    }

    #[test]
    fn unscripted_url_fails_showing_url_with_one_call() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        match cache.fetch(URL, None, false) {
            Ok(_) => panic!("unscripted url serves"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("cannot fetch"),
                    "miss is download error: {text}"
                );
                assert!(text.contains(URL), "error shows the url: {text}");
                assert!(
                    text.contains("no scripted body"),
                    "error shows the miss: {text}"
                );
            }
        }
        assert_eq!(driver::http::calls(URL), 1, "one download attempt per miss");
    }

    #[test]
    fn miss_then_hit_pins_one_download_plus_zero_extra() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        driver::http::serve(URL, b"1.2.3");
        let (sender, receiver) = crossbeam_channel::unbounded();
        let cache = FetchCache::new(&test_roots(dir.path()), sender);
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                assert_eq!(
                    driver::fs::read(handle.canonical()).unwrap(),
                    b"1.2.3".to_vec()
                );
            }
            Err(error) => panic!("miss downloads: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                Event::FetchStarted {
                    url: URL.to_string(),
                },
                Event::FetchDownloaded {
                    url: URL.to_string(),
                    bytes: 5,
                },
            ]
        );
        assert_eq!(driver::http::calls(URL), 1, "one download per miss");
        let (sender, receiver) = crossbeam_channel::unbounded();
        let cache = FetchCache::new(&test_roots(dir.path()), sender);
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                assert_eq!(
                    driver::fs::read(handle.canonical()).unwrap(),
                    b"1.2.3".to_vec()
                );
            }
            Err(error) => panic!("hit serves: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                Event::FetchStarted {
                    url: URL.to_string(),
                },
                Event::FetchCached {
                    url: URL.to_string(),
                    bytes: 5,
                },
            ]
        );
        assert_eq!(
            driver::http::calls(URL),
            1,
            "offline hit adds zero downloads"
        );
        let wrong = Sha::new("0".repeat(64)).unwrap();
        match cache.fetch(URL, Some(wrong), false) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(URL), "error shows the url: {text}");
                assert!(
                    text.contains("sha256 mismatch"),
                    "error reports loss: {text}"
                );
            }
        }
        assert_eq!(
            driver::http::calls(URL),
            1,
            "user-sha failure downloads nothing"
        );
    }

    #[test]
    fn download_reports_climbing_bytes_across_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        driver::http::clear();
        let raw: Vec<u8> = (0..256 * 1024).map(|index| (index % 251) as u8).collect();
        driver::http::serve(URL, &raw);
        let (sender, receiver) = crossbeam_channel::unbounded();
        let cache = FetchCache::new(&test_roots(dir.path()), sender);
        match cache.fetch(URL, None, false) {
            Ok(_) => {}
            Err(error) => panic!("large miss downloads: {error}"),
        }
        let mut seen: Vec<u64> = Vec::new();
        for event in receiver.try_iter() {
            if let Event::FetchProgress {
                bytes_so_far,
                total,
                ..
            } = event
            {
                assert_eq!(total, Some(raw.len() as u64), "progress keeps the total");
                seen.push(bytes_so_far);
            }
        }
        assert!(seen.len() >= 2, "chunks report twice: {}", seen.len());
        for pair in seen.windows(2) {
            assert!(pair[0] < pair[1], "bytes climb: {}", seen.len());
        }
    }

    #[test]
    fn pooled_paths_agree_for_identical_content() {
        use crate::blob::{BlobSource, BlobStore};

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = StoreRoots {
            config_base: dir.path().join("config"),
            temp_base: dir.path().join("temp"),
            project_root: dir.path().join("project"),
        };
        let store = {
            let (sender, _) = crossbeam_channel::unbounded();
            BlobStore::new(&roots, sender)
        };
        let small = b"shared pool bytes".to_vec();
        let via_put = store.put(BlobSource::Bytes(&small)).unwrap();
        let source_path = dir.path().join("source.bin");
        driver::fs::write(&source_path, &small).unwrap();
        let source = ResourceHandle::new(dir.path(), source_path, Sha::hash(&small)).unwrap();
        let via_source = store.put(BlobSource::Handle(&source)).unwrap();
        assert_eq!(
            via_source, via_put,
            "trusted source keeps verified identity"
        );
        let large: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(large.len() > STREAM_BUF_BYTES);
        let large_put = store.put(BlobSource::Bytes(&large)).unwrap();
        let large_path = dir.path().join("large.bin");
        driver::fs::write(&large_path, &large).unwrap();
        let large_source = ResourceHandle::new(dir.path(), large_path, Sha::hash(&large)).unwrap();
        let large_sourced = store.put(BlobSource::Handle(&large_source)).unwrap();
        assert_eq!(
            large_sourced.sha256(),
            large_put.sha256(),
            "sourced bytes keep content hash"
        );
        assert_eq!(
            large_put, large_sourced,
            "both shapes keep one verified identity"
        );
        for handle in [&large_put, &large_sourced] {
            let mut found = Vec::new();
            store.open(handle).unwrap().read_to_end(&mut found).unwrap();
            assert_eq!(found, large, "pooled bytes round-trip");
        }
    }

    #[test]
    fn fetch_reports_write_too_large() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        driver::http::clear();
        let cache = file_cache(dir.path());
        driver::http::serve(URL, b"oversized");
        guard.fail_writes(std::io::ErrorKind::FileTooLarge);
        match cache.fetch(URL, None, false) {
            Ok(_) => panic!("oversized file passes"),
            Err(FetchError::Write { url, fault }) => {
                assert_eq!(url, URL, "pending keeps the url");
                assert!(
                    matches!(fault, crate::faults::AccessFault::FileTooLarge),
                    "pending keeps the fault"
                );
            }
            Err(error) => panic!("wrong pending variant: {error}"),
        }
    }
}
