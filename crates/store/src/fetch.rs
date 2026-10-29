//! Content-addressed fetch cache for remote bytes.

use std::path::PathBuf;
use std::sync::Arc;

use confit_core::error::{
    ContentError, ContextualCause, ContextualResult, ErrorCause, WithContext, contextual,
};
use confit_core::progress::{Event, ProgressSender};
use confit_core::sha::Sha;

use crate::handles::ResourceHandle;
use crate::resources::Resources;
use confit_driver as driver;

/// Body cap passed per call into the driver download.
const BODY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

/// Progress step for in-flight download events.
const PROGRESS_STEP_BYTES: u64 = 64 * 1024;

/// Index extension for url entries under the cache folder.
const INDEX_EXT: &str = "url";

/// File-backed fetch cache for remote bytes.
#[derive(Clone, Debug)]
pub struct FetchCache {
    resources: Arc<Resources>,
    progress: ProgressSender,
}

impl FetchCache {
    /// Build one file-backed fetch cache over shared resources.
    pub fn new(resources: Arc<Resources>, progress: ProgressSender) -> Self {
        Self {
            resources,
            progress,
        }
    }

    /// Derive the index entry path for one URL.
    fn index_path(&self, url: &str) -> PathBuf {
        let name = format!("{}.{INDEX_EXT}", Sha::hash(url.as_bytes()).hex());
        self.resources.cache_dir().join(name)
    }
}

impl FetchCache {
    /// Fetch one URL into a resource handle.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FetchError`] for failing downloads.
    /// - [`ErrorCause::ContentHashMismatch`] for sha mismatches.
    /// - [`FilesystemError`] for cache write faults.
    pub fn fetch(
        &self,
        url: &str,
        expected_sha: Option<Sha>,
        re_fetch: bool,
    ) -> ContextualResult<ResourceHandle> {
        Fetch::new(self, url, expected_sha.as_ref()).run(re_fetch)
    }
}

/// Metered reader reporting downloaded chunk sizes.
struct Meter<R, F> {
    /// The wrapped download stream.
    inner: R,
    /// The progress hook by chunk size.
    on_read: F,
}

impl<R: std::io::Read, F: FnMut(u64)> std::io::Read for Meter<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        (self.on_read)(read as u64);
        Ok(read)
    }
}

/// One fetch walk.
struct Fetch<'a> {
    /// The cache under fetching.
    cache: &'a FetchCache,
    /// The url under fetching.
    url: &'a str,
    /// The user sha under checking.
    expected: Option<&'a Sha>,
    /// The download total under reporting.
    total: Option<u64>,
    /// The bytes seen so far.
    bytes: u64,
    /// The bytes reported so far.
    reported: u64,
}

impl<'a> Fetch<'a> {
    /// Build one fetch walk.
    fn new(cache: &'a FetchCache, url: &'a str, expected: Option<&'a Sha>) -> Self {
        Self {
            cache,
            url,
            expected,
            total: None,
            bytes: 0,
            reported: 0,
        }
    }

    /// Run one walk into a resource handle.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FetchError`] for failing downloads.
    /// - [`ErrorCause::ContentHashMismatch`] for sha mismatches.
    /// - [`FilesystemError`] for cache write faults.
    fn run(&mut self, re_fetch: bool) -> ContextualResult<ResourceHandle> {
        let _ = self.cache.progress.send(Event::FetchStarted {
            url: self.url.to_owned(),
        });
        if !re_fetch && let Some(hit) = self.hit() {
            return hit;
        }
        self.download()
    }

    /// Serve one cached hit with the cached progress event.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for hits failing the user sha.
    fn hit(&self) -> Option<ContextualResult<ResourceHandle>> {
        let hit = self.lookup()?;
        let ResourceHandle::Cache(sha) = &hit else {
            return None;
        };
        let path = self.cache.resources.cache_path(sha);
        let bytes = std::fs::metadata(&path)
            .map(|facts| usize::try_from(facts.len()).unwrap_or(0))
            .unwrap_or(0);
        let _ = self.cache.progress.send(Event::FetchCached {
            url: self.url.to_owned(),
            bytes,
        });
        match self.check(sha) {
            Ok(()) => Some(Ok(hit)),
            Err(error) => Some(Err(error)),
        }
    }

    /// Find one cached handle with a present content file.
    fn lookup(&self) -> Option<ResourceHandle> {
        let stored = std::fs::read_to_string(self.cache.index_path(self.url)).ok()?;
        let wanted = Sha::new(stored.trim()).ok()?;
        if !self.cache.resources.cache_path(&wanted).exists() {
            return None;
        }
        Some(ResourceHandle::Cache(wanted))
    }

    /// Download one URL body with the downloaded progress event.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FetchError`] for failing downloads.
    /// - [`ErrorCause::ContentHashMismatch`] for sha mismatches.
    /// - [`FilesystemError`] for cache write faults.
    fn download(&mut self) -> ContextualResult<ResourceHandle> {
        let (reader, total) = driver::http::download(self.url, BODY_LIMIT_BYTES)?;
        self.total = total;
        let resources = self.cache.resources.clone();
        let metered = Meter {
            inner: reader,
            on_read: |bytes: u64| {
                self.bytes += bytes;
                self.report();
            },
        };
        let handle = resources.store(metered).with_url(self.url)?;
        let sha = match handle {
            ResourceHandle::Cache(sha) => sha,
            ResourceHandle::Project(..) => {
                return Err(contextual(ContentError::ProjectHandleInCache).with_url(self.url));
            }
        };
        self.check(&sha)?;
        self.write_index(&sha)?;
        let _ = self.cache.progress.send(Event::FetchDownloaded {
            url: self.url.to_owned(),
            bytes: usize::try_from(self.bytes).unwrap_or(0),
        });
        Ok(ResourceHandle::Cache(sha))
    }

    /// Report one progress step.
    fn report(&mut self) {
        if self.bytes - self.reported >= PROGRESS_STEP_BYTES {
            self.reported = self.bytes;
            let _ = self.cache.progress.send(Event::FetchProgress {
                url: self.url.to_owned(),
                bytes_so_far: self.bytes,
                total: self.total,
            });
        }
    }

    /// Write the url index entry for one content digest.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for cache write faults.
    fn write_index(&self, digest: &Sha) -> ContextualResult<()> {
        let index = self.cache.index_path(self.url);
        driver::fs::create_parent(&index).with_cache_path(&index)?;
        std::fs::write(&index, digest.hex().as_bytes())
            .map_err(ContextualCause::from)
            .with_cache_path(&index)?;
        Ok(())
    }

    /// Check one digest against the user sha.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for mismatches.
    fn check(&self, actual: &Sha) -> ContextualResult<()> {
        let Some(wanted) = self.expected else {
            return Ok(());
        };
        if actual != wanted {
            return Err(contextual(ErrorCause::ContentHashMismatch {
                expected: (*wanted).clone(),
                found: actual.clone(),
            })
            .with_url(self.url));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use crate::StoreRoots;
    use crate::pool::PoolStore;
    use confit_core::error::ErrorCause;
    use confit_driver as driver;

    const URL: &str = "https://example.com/tool.bin";
    const FILE_URL: &str = "https://example.com/tool.tar.gz";

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            ..Default::default()
        }
    }

    fn file_cache(dir: &Path) -> FetchCache {
        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        FetchCache::new(resources, sender)
    }

    fn index_file(dir: &Path, url: &str) -> PathBuf {
        dir.join("temp").join("cache").join(format!(
            "{}.{}",
            Sha::hash(url.as_bytes()).hex(),
            INDEX_EXT
        ))
    }

    fn content_file(dir: &Path, sha_hex: &str) -> PathBuf {
        dir.join("temp").join("cache").join(sha_hex)
    }

    fn cached_sha(handle: &ResourceHandle) -> Sha {
        match handle {
            ResourceHandle::Cache(sha) => sha.clone(),
            ResourceHandle::Project(..) => panic!("fetched bytes land cached"),
        }
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
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        assert_eq!(driver::http::calls(URL), 1, "seeding downloads once");
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                match std::fs::read(content_file(dir.path(), &sha.hex())) {
                    Ok(found) => assert_eq!(found, b"1.2.3".to_vec()),
                    Err(error) => panic!("seeded body load: {error}"),
                }
                assert_eq!(driver::http::calls(URL), 1, "hit makes no download");
            }
            Err(error) => panic!("seeded bytes serve: {error}"),
        }
    }

    #[test]
    fn file_hit_gave_path_under_cache() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, FILE_URL, b"binary");
        match cache.fetch(FILE_URL, None, false) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                let path = content_file(dir.path(), &sha.hex());
                assert!(
                    path.starts_with(dir.path().join("temp").join("cache")),
                    "cache path stays under cache: {}",
                    path.display()
                );
                match std::fs::read(&path) {
                    Ok(found) => assert_eq!(found, b"binary".to_vec()),
                    Err(error) => panic!("cached file load: {error}"),
                }
            }
            Err(error) => panic!("cached file serves: {error}"),
        }
    }

    #[test]
    fn missing_content_heals_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        let wanted = Sha::hash(b"genuine");
        std::fs::remove_file(content_file(dir.path(), &wanted.hex())).unwrap();
        match cache.fetch(URL, None, false) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                assert_eq!(sha, wanted, "content loss keeps the digest");
                match std::fs::read(content_file(dir.path(), &sha.hex())) {
                    Ok(found) => assert_eq!(found, b"genuine".to_vec(), "content loss redownloads"),
                    Err(error) => panic!("healed entry load: {error}"),
                }
                assert_eq!(driver::http::calls(URL), 2, "content loss redownloads once");
            }
            Err(error) => panic!("healed entry redownloads: {error}"),
        }
    }

    #[test]
    fn missing_index_parsed_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        std::fs::remove_file(index_file(dir.path(), URL)).unwrap();
        match cache.fetch(URL, None, false) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, b"genuine".to_vec(), "index loss redownloads");
                assert_eq!(driver::http::calls(URL), 2, "index loss redownloads once");
            }
            Err(error) => panic!("index-less entry redownloads: {error}"),
        }
    }

    #[test]
    fn corrupt_index_parsed_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        std::fs::write(index_file(dir.path(), URL), b"not-a-hex-digest").unwrap();
        match cache.fetch(URL, None, false) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, b"genuine".to_vec(), "corrupt index redownloads");
            }
            Err(error) => panic!("corrupt index redownloads: {error}"),
        }
    }

    #[test]
    fn layout_uses_content_address_with_url_index() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        assert_eq!(
            content_file(dir.path(), &wanted.hex()),
            dir.path().join("temp").join("cache").join(wanted.hex())
        );
        assert!(content_file(dir.path(), &wanted.hex()).exists());
        let index = index_file(dir.path(), URL);
        assert_eq!(
            index,
            dir.path().join("temp").join("cache").join(format!(
                "{}.{}",
                Sha::hash(URL.as_bytes()).hex(),
                INDEX_EXT
            ))
        );
        assert_eq!(std::fs::read_to_string(&index).unwrap(), wanted.hex());
        assert_no_sha_suffix_files(&dir.path().join("temp"));
    }

    #[test]
    fn hit_reports_started_and_cached_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        let cache = FetchCache::new(resources, sender);
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
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        let cache = FetchCache::new(resources, sender);
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
        driver::http::clear();
        driver::http::serve(URL, b"1.2.3");
        driver::http::serve(FILE_URL, b"binary");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        let cache = FetchCache::new(resources, sender);
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
                if std::fs::metadata(&path).unwrap().is_dir() {
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
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        seed(&cache, FILE_URL, b"binary");
        let wrong = "0".repeat(64);
        match cache.fetch(URL, Some(Sha::new(wrong.clone()).unwrap()), false) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                assert!(
                    matches!(error.cause(), ErrorCause::ContentHashMismatch { .. }),
                    "mismatch keeps the cause: {error}"
                );
                assert!(
                    matches!(error.context.url.as_deref(), Some(URL)),
                    "mismatch keeps the url: {error}"
                );
            }
        }
        match cache.fetch(FILE_URL, Some(Sha::new(wrong.clone()).unwrap()), false) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                assert!(
                    matches!(error.cause(), ErrorCause::ContentHashMismatch { .. }),
                    "mismatch keeps the cause: {error}"
                );
                assert!(
                    matches!(error.context.url.as_deref(), Some(FILE_URL)),
                    "mismatch keeps the url: {error}"
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
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        match cache.fetch(URL, Some(wanted.clone()), false) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                assert_eq!(sha, wanted);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("good user sha serves: {error}"),
        }
    }

    #[test]
    fn re_fetch_forces_download_past_valid_entry() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
                assert_eq!(driver::http::calls(URL), 1, "hit makes no download");
            }
            Err(error) => panic!("cached read serves: {error}"),
        }
        match cache.fetch(URL, None, true) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, b"1.2.3".to_vec(), "forced fetch redownloads");
                assert_eq!(driver::http::calls(URL), 2, "forced fetch downloads once");
            }
            Err(error) => panic!("forced fetch downloads: {error}"),
        }
    }

    #[test]
    fn bytes_hit_streams_large_entry() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > 8192, "entry spans several store chunks");
        seed(&cache, URL, &raw);
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                match &handle {
                    ResourceHandle::Cache(sha) => assert_eq!(sha, &Sha::hash(&raw)),
                    ResourceHandle::Project(..) => panic!("fetched bytes land cached"),
                }
                let sha = cached_sha(&handle);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("large hit serves: {error}"),
        }
    }

    #[test]
    fn scripted_failure_parsed_as_download_error_showing_url() {
        use confit_core::error::FetchError;

        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        driver::http::fail(URL, FetchError::DownloadFailed);
        match cache.fetch(URL, None, false) {
            Ok(_) => panic!("scripted failure passes"),
            Err(error) => {
                assert!(matches!(
                    error.cause(),
                    confit_core::error::ErrorCause::FetchError(FetchError::DownloadFailed)
                ));
                assert!(
                    matches!(error.context.url.as_deref(), Some(URL)),
                    "failure keeps the url: {error}"
                );
            }
        }
        assert_eq!(driver::http::calls(URL), 1, "one download attempt per miss");
        assert!(
            cache.fetch(URL, None, false).is_err(),
            "scripted retry fails"
        );
        assert_eq!(driver::http::calls(URL), 2, "retry downloads once more");
    }

    #[test]
    fn miss_then_hit_pins_one_download_plus_zero_extra() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        driver::http::serve(URL, b"1.2.3");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                assert_eq!(
                    std::fs::read(content_file(dir.path(), &sha.hex())).unwrap(),
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
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None, false) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                assert_eq!(
                    std::fs::read(content_file(dir.path(), &sha.hex())).unwrap(),
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
                assert!(
                    matches!(error.cause(), ErrorCause::ContentHashMismatch { .. }),
                    "mismatch keeps the cause: {error}"
                );
                assert!(
                    matches!(error.context.url.as_deref(), Some(URL)),
                    "mismatch keeps the url: {error}"
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
        driver::http::clear();
        let raw: Vec<u8> = (0..256 * 1024).map(|index| (index % 251) as u8).collect();
        driver::http::serve(URL, &raw);
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools));
        let cache = FetchCache::new(resources, sender);
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
}
