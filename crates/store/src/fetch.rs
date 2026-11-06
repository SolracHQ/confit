//! Content-addressed fetch cache for remote bytes.

use std::path::PathBuf;
use std::sync::Arc;

use confit_core::error::{ContextualCause, ContextualResult, ErrorCause, WithContext, contextual};
use confit_core::resource_ref::ResourceRef;
use confit_core::sha::Sha;
use confit_events::ToUi;
use crossbeam_channel::Sender;

use crate::resources::Resources;
use confit_driver as driver;

/// The body limit for one driver download.
const BODY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

/// The byte interval between progress events during a download.
const PROGRESS_STEP_BYTES: usize = 64 * 1024;

/// The file extension for URL index entries in the cache folder.
const INDEX_EXT: &str = "url";

/// A file-backed cache of remote bytes keyed by URL and content hash.
#[derive(Clone, Debug)]
pub struct FetchCache {
    resources: Arc<Resources>,
    to_ui: Sender<ToUi>,
}

impl FetchCache {
    /// Builds a cache over shared resources.
    pub fn new(resources: Arc<Resources>, to_ui: Sender<ToUi>) -> Self {
        Self { resources, to_ui }
    }

    /// Returns the index path under the cache folder.
    fn index_path(&self, url: &str) -> PathBuf {
        let name = format!("{}.{INDEX_EXT}", Sha::hash(url.as_bytes()).hex());
        self.resources.cache_dir().join(name)
    }
}

impl FetchCache {
    /// Downloads one URL and returns its trusted hash.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FetchError`] when a download fails.
    /// - [`ErrorCause::ContentHashMismatch`] when the hash mismatches.
    /// - [`FilesystemError`] when a cache write fails.
    pub fn fetch(&self, url: &str, expected_sha: Option<Sha>) -> ContextualResult<ResourceRef> {
        Fetch::new(self, url, expected_sha.as_ref()).run()
    }
}

/// A reader that counts each chunk and reports its size.
struct Meter<R, F> {
    /// The wrapped download stream.
    inner: R,
    /// The callback that receives each chunk size.
    on_read: F,
}

impl<R: std::io::Read, F: FnMut(usize)> std::io::Read for Meter<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        (self.on_read)(read);
        Ok(read)
    }
}

/// One pending download.
struct Fetch<'a> {
    /// The fetch cache.
    cache: &'a FetchCache,
    /// The URL being fetched.
    url: &'a str,
    /// The expected hash from the caller.
    expected: Option<&'a Sha>,
    /// The total download size.
    total: Option<usize>,
    /// The bytes read so far.
    bytes: usize,
    /// The bytes reported so far.
    reported: usize,
}

impl<'a> Fetch<'a> {
    /// Builds a walk that fetches one URL.
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

    /// Runs the walk and returns the fetched hash.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FetchError`] when a download fails.
    /// - [`ErrorCause::ContentHashMismatch`] when the hash mismatches.
    /// - [`FilesystemError`] when a cache write fails.
    fn run(&mut self) -> ContextualResult<ResourceRef> {
        let _ = self.cache.to_ui.send(ToUi::FetchStarted {
            url: self.url.to_owned(),
        });
        if let Some(hit) = self.hit() {
            return hit;
        }
        self.download()
    }

    /// Returns the cached hash when the index points at present bytes.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the hit fails the user hash.
    fn hit(&self) -> Option<ContextualResult<ResourceRef>> {
        let hit = self.lookup()?;
        let _ = self.cache.to_ui.send(ToUi::FetchFinished {
            url: self.url.to_owned(),
            cached: true,
        });
        match self.check(hit.sha()) {
            Ok(()) => Some(Ok(hit)),
            Err(error) => Some(Err(error)),
        }
    }

    /// Returns the trusted hash when the URL index points at present bytes.
    fn lookup(&self) -> Option<ResourceRef> {
        let stored = std::fs::read_to_string(self.cache.index_path(self.url)).ok()?;
        let wanted = Sha::new(stored.trim()).ok()?;
        if !self.cache.resources.cache_path(&wanted).exists() {
            return None;
        }
        Some(ResourceRef::new(wanted))
    }

    /// Downloads one URL body and writes its progress events.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FetchError`] when a download fails.
    /// - [`ErrorCause::ContentHashMismatch`] when the hash mismatches.
    /// - [`FilesystemError`] when a cache write fails.
    fn download(&mut self) -> ContextualResult<ResourceRef> {
        let (reader, total) = driver::http::download(self.url, BODY_LIMIT_BYTES)?;
        self.total = total.map(|known| usize::try_from(known).unwrap_or(usize::MAX));
        let resources = self.cache.resources.clone();
        let metered = Meter {
            inner: reader,
            on_read: |bytes: usize| {
                self.bytes += bytes;
                self.report();
            },
        };
        let handle = resources.store(metered).with_url(self.url)?;
        self.check(handle.sha())?;
        self.write_index(handle.sha())?;
        let _ = self.cache.to_ui.send(ToUi::FetchFinished {
            url: self.url.to_owned(),
            cached: false,
        });
        Ok(handle)
    }

    /// Writes one progress event.
    fn report(&mut self) {
        if self.bytes - self.reported >= PROGRESS_STEP_BYTES {
            self.reported = self.bytes;
            let _ = self.cache.to_ui.send(ToUi::FetchProgress {
                url: self.url.to_owned(),
                done: self.bytes,
                total: self.total,
            });
        }
    }

    /// Writes the index entry that maps one URL to its content hash.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the index fails to write.
    fn write_index(&self, digest: &Sha) -> ContextualResult<()> {
        let index = self.cache.index_path(self.url);
        driver::fs::create_parent(&index).with_cache_path(&index)?;
        std::fs::write(&index, digest.hex().as_bytes())
            .map_err(ContextualCause::from)
            .with_cache_path(&index)?;
        Ok(())
    }

    /// Checks one digest against the expected hash.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the digest mismatches.
    fn check(&self, actual: &Sha) -> ContextualResult<()> {
        let Some(wanted) = self.expected else {
            return Ok(());
        };
        if actual != wanted {
            return Err(contextual(ErrorCause::ContentHashMismatch {
                expected: *wanted,
                found: *actual,
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
        let resources = Arc::new(Resources::new(&roots, pools, sender.clone()));
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

    fn cached_sha(handle: &ResourceRef) -> Sha {
        *handle.sha()
    }

    fn seed(cache: &FetchCache, url: &str, body: &[u8]) -> ResourceRef {
        driver::http::serve(url, body);
        match cache.fetch(url, None) {
            Ok(handle) => handle,
            Err(error) => panic!("the seeded body fails to serve: {error}"),
        }
    }

    #[test]
    fn fetch_hit_serves_a_seeded_entry() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        assert_eq!(driver::http::calls(URL), 1, "the seed downloads once");
        match cache.fetch(URL, None) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                match std::fs::read(content_file(dir.path(), &sha.hex())) {
                    Ok(found) => assert_eq!(found, b"1.2.3".to_vec()),
                    Err(error) => panic!("the seeded body fails to load: {error}"),
                }
                assert_eq!(driver::http::calls(URL), 1, "the hit makes no download");
            }
            Err(error) => panic!("the seeded bytes fail to serve: {error}"),
        }
    }

    #[test]
    fn fetch_file_hit_lands_under_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, FILE_URL, b"binary");
        match cache.fetch(FILE_URL, None) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                let path = content_file(dir.path(), &sha.hex());
                assert!(
                    path.starts_with(dir.path().join("temp").join("cache")),
                    "the cache path is under the cache: {}",
                    path.display()
                );
                match std::fs::read(&path) {
                    Ok(found) => assert_eq!(found, b"binary".to_vec()),
                    Err(error) => panic!("the cached file fails to load: {error}"),
                }
            }
            Err(error) => panic!("the cached file fails to serve: {error}"),
        }
    }

    #[test]
    fn fetch_redownloads_absent_content_as_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        let wanted = Sha::hash(b"genuine");
        std::fs::remove_file(content_file(dir.path(), &wanted.hex())).unwrap();
        match cache.fetch(URL, None) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                assert_eq!(sha, wanted, "the content loss keeps the digest");
                match std::fs::read(content_file(dir.path(), &sha.hex())) {
                    Ok(found) => assert_eq!(
                        found,
                        b"genuine".to_vec(),
                        "the content loss redownloads the bytes"
                    ),
                    Err(error) => panic!("the healed entry fails to load: {error}"),
                }
                assert_eq!(
                    driver::http::calls(URL),
                    2,
                    "the content loss makes two downloads"
                );
            }
            Err(error) => panic!("the healed entry fails to redownload: {error}"),
        }
    }

    #[test]
    fn fetch_treats_an_absent_index_as_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        std::fs::remove_file(index_file(dir.path(), URL)).unwrap();
        match cache.fetch(URL, None) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(
                    found,
                    b"genuine".to_vec(),
                    "the index loss redownloads the bytes"
                );
                assert_eq!(
                    driver::http::calls(URL),
                    2,
                    "the index loss makes two downloads"
                );
            }
            Err(error) => panic!("the index-less entry fails to redownload: {error}"),
        }
    }

    #[test]
    fn fetch_treats_a_corrupt_index_as_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"genuine");
        std::fs::write(index_file(dir.path(), URL), b"not-a-hex-digest").unwrap();
        match cache.fetch(URL, None) {
            Ok(refetched) => {
                let sha = cached_sha(&refetched);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(
                    found,
                    b"genuine".to_vec(),
                    "the corrupt index redownloads the bytes"
                );
            }
            Err(error) => panic!("the corrupt index fails to redownload: {error}"),
        }
    }

    #[test]
    fn layout_uses_a_content_address_with_a_url_index() {
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
    fn fetch_hit_reports_started_and_cached() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools, sender.clone()));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None) {
            Ok(_) => {
                let events: Vec<ToUi> = receiver.try_iter().collect();
                assert_eq!(
                    events,
                    vec![
                        ToUi::FetchStarted {
                            url: URL.to_string(),
                        },
                        ToUi::FetchFinished {
                            url: URL.to_string(),
                            cached: true,
                        },
                    ]
                );
                assert_eq!(driver::http::calls(URL), 1, "the hit makes no download");
            }
            Err(error) => panic!("the hit fails to report progress: {error}"),
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
        let resources = Arc::new(Resources::new(&roots, pools, sender.clone()));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None) {
            Ok(_) => {}
            Err(error) => panic!("the first miss fails to download: {error}"),
        }
        match cache.fetch(FILE_URL, None) {
            Ok(_) => {}
            Err(error) => panic!("the second miss fails to download: {error}"),
        }
        match cache.fetch(URL, None) {
            Ok(_) => {}
            Err(error) => panic!("the repeat hit fails to serve: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                ToUi::FetchStarted {
                    url: URL.to_string(),
                },
                ToUi::FetchFinished {
                    url: URL.to_string(),
                    cached: false,
                },
                ToUi::FetchStarted {
                    url: FILE_URL.to_string(),
                },
                ToUi::FetchFinished {
                    url: FILE_URL.to_string(),
                    cached: false,
                },
                ToUi::FetchStarted {
                    url: URL.to_string(),
                },
                ToUi::FetchFinished {
                    url: URL.to_string(),
                    cached: true,
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
                        "the cache holds no sha-suffixed file: {}",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn fetch_reports_a_user_hash_mismatch_after_a_hit() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        seed(&cache, FILE_URL, b"binary");
        let other_hash = "0".repeat(64);
        match cache.fetch(URL, Some(Sha::new(other_hash.clone()).unwrap())) {
            Ok(_) => panic!("expected a user hash mismatch"),
            Err(error) => {
                assert!(
                    matches!(error.cause(), ErrorCause::ContentHashMismatch { .. }),
                    "the mismatch keeps its cause: {error}"
                );
                assert!(
                    matches!(error.context.url.as_deref(), Some(URL)),
                    "the mismatch keeps the url: {error}"
                );
            }
        }
        match cache.fetch(FILE_URL, Some(Sha::new(other_hash.clone()).unwrap())) {
            Ok(_) => panic!("expected a user hash mismatch"),
            Err(error) => {
                assert!(
                    matches!(error.cause(), ErrorCause::ContentHashMismatch { .. }),
                    "the mismatch keeps its cause: {error}"
                );
                assert!(
                    matches!(error.context.url.as_deref(), Some(FILE_URL)),
                    "the mismatch keeps the url: {error}"
                );
            }
        }
        assert_eq!(
            driver::http::calls(URL),
            1,
            "the user hash failure makes no download"
        );
    }

    #[test]
    fn fetch_serves_a_matching_user_hash_after_a_hit() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        seed(&cache, URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        match cache.fetch(URL, Some(wanted)) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                assert_eq!(sha, wanted);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("the user hash fails to serve: {error}"),
        }
    }

    #[test]
    fn fetch_hit_streams_a_large_entry() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > 8192, "the entry spans several store chunks");
        seed(&cache, URL, &raw);
        match cache.fetch(URL, None) {
            Ok(handle) => {
                assert_eq!(handle.sha(), &Sha::hash(&raw));
                let sha = cached_sha(&handle);
                let found = std::fs::read(content_file(dir.path(), &sha.hex())).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("the large hit fails to serve: {error}"),
        }
    }

    #[test]
    fn fetch_reports_a_scripted_failure_with_its_url() {
        use confit_core::error::FetchError;

        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        let cache = file_cache(dir.path());
        driver::http::fail(URL, FetchError::DownloadFailed);
        match cache.fetch(URL, None) {
            Ok(_) => panic!("expected a scripted failure"),
            Err(error) => {
                assert!(matches!(
                    error.cause(),
                    ErrorCause::FetchError(FetchError::DownloadFailed)
                ));
                assert!(
                    matches!(error.context.url.as_deref(), Some(URL)),
                    "the failure keeps the url: {error}"
                );
            }
        }
        assert_eq!(
            driver::http::calls(URL),
            1,
            "the miss makes one download attempt"
        );
        assert!(
            cache.fetch(URL, None).is_err(),
            "the retry reports the scripted failure"
        );
        assert_eq!(
            driver::http::calls(URL),
            2,
            "the retry makes two download attempts"
        );
    }

    #[test]
    fn fetch_keeps_one_download_across_a_miss_and_a_hit() {
        let dir = tempfile::tempdir().unwrap();
        driver::http::clear();
        driver::http::serve(URL, b"1.2.3");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools, sender.clone()));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                assert_eq!(
                    std::fs::read(content_file(dir.path(), &sha.hex())).unwrap(),
                    b"1.2.3".to_vec()
                );
            }
            Err(error) => panic!("the miss fails to download: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                ToUi::FetchStarted {
                    url: URL.to_string(),
                },
                ToUi::FetchFinished {
                    url: URL.to_string(),
                    cached: false,
                },
            ]
        );
        assert_eq!(driver::http::calls(URL), 1, "the miss makes one download");
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = Arc::new(Resources::new(&roots, pools, sender.clone()));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None) {
            Ok(handle) => {
                let sha = cached_sha(&handle);
                assert_eq!(
                    std::fs::read(content_file(dir.path(), &sha.hex())).unwrap(),
                    b"1.2.3".to_vec()
                );
            }
            Err(error) => panic!("the hit fails to serve: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(
            events,
            vec![
                ToUi::FetchStarted {
                    url: URL.to_string(),
                },
                ToUi::FetchFinished {
                    url: URL.to_string(),
                    cached: true,
                },
            ]
        );
        assert_eq!(
            driver::http::calls(URL),
            1,
            "the offline hit makes no download"
        );
        let other_sha = Sha::new("0".repeat(64)).unwrap();
        match cache.fetch(URL, Some(other_sha)) {
            Ok(_) => panic!("expected a user hash mismatch"),
            Err(error) => {
                assert!(
                    matches!(error.cause(), ErrorCause::ContentHashMismatch { .. }),
                    "the mismatch keeps its cause: {error}"
                );
                assert!(
                    matches!(error.context.url.as_deref(), Some(URL)),
                    "the mismatch keeps the url: {error}"
                );
            }
        }
        assert_eq!(
            driver::http::calls(URL),
            1,
            "the user hash failure makes no download"
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
        let resources = Arc::new(Resources::new(&roots, pools, sender.clone()));
        let cache = FetchCache::new(resources, sender);
        match cache.fetch(URL, None) {
            Ok(_) => {}
            Err(error) => panic!("the large miss fails to download: {error}"),
        }
        let mut seen: Vec<usize> = Vec::new();
        for event in receiver.try_iter() {
            if let ToUi::FetchProgress { done, total, .. } = event {
                assert_eq!(total, Some(raw.len()), "the progress keeps the total");
                seen.push(done);
            }
        }
        assert!(
            seen.len() >= 2,
            "the chunks report at least twice: {}",
            seen.len()
        );
        for pair in seen.windows(2) {
            assert!(pair[0] < pair[1], "the bytes climb: {}", seen.len());
        }
    }
}
