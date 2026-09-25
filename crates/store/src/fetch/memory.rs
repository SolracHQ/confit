//! Memory
//!
//! Memory fetch cache for tests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::handles::{FetchHandle, Sha, TrustedHandle};
use confit_core::progress::{Event, ProgressSender};

use super::FetchCache;

/// Memory fetch cache for tests.
///
/// Bodies ride a content map behind a url index with call
/// counts proving offline hits.
#[derive(Debug, Default)]
pub struct MemoryFetchCache {
    base: PathBuf,
    index: Mutex<HashMap<String, String>>,
    bodies: Mutex<HashMap<String, Vec<u8>>>,
    calls: Mutex<HashMap<String, usize>>,
}

impl MemoryFetchCache {
    /// Builds one memory fetch cache holding the cache base.
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            index: Mutex::new(HashMap::new()),
            bodies: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
        }
    }

    /// Seeds one URL body stub.
    ///
    /// The stub lands behind the url index like a download.
    pub fn insert(&self, url: &str, body: &[u8]) {
        let digest = Sha::hash(body).hex();
        let mut index = match self.index.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        index.insert(url.to_string(), digest.clone());
        drop(index);
        let mut bodies = match self.bodies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        bodies.insert(digest, body.to_vec());
    }

    /// Reads download counts for one URL.
    pub fn calls(&self, url: &str) -> usize {
        let guard = match self.calls.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(url).copied().unwrap_or(0)
    }

    /// Derives the cache file path for one content digest.
    fn cache_path(&self, sha_hex: &str) -> PathBuf {
        self.base.join(sha_hex)
    }

    /// Reads cached bytes passing the index digest check.
    ///
    /// Tampered entries read as misses.
    fn lookup(&self, url: &str) -> Option<(Sha, Vec<u8>)> {
        let wanted = {
            let guard = match self.index.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.get(url).cloned()?
        };
        let wanted = wanted.trim().to_lowercase();
        if !is_hex64(&wanted) {
            return None;
        }
        let body = {
            let guard = match self.bodies.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            guard.get(&wanted).cloned()?
        };
        let actual = Sha::hash(&body);
        if actual != Sha::new(wanted).ok()? {
            return None;
        }
        Some((actual, body))
    }

    /// Serves one cached hit with the cached progress event.
    ///
    /// Misses answer none so the caller downloads.
    ///
    /// # Errors
    ///
    /// Cached hits failing the user sha fail as plan errors
    /// naming the URL. Unwritable cache files fail as plan
    /// errors naming the path.
    fn serve_hit(
        &self,
        url: &str,
        expected_sha: Option<&Sha>,
        progress: Option<&ProgressSender>,
    ) -> Option<Result<FetchHandle>> {
        let (sha, stored) = self.lookup(url)?;
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchCached {
                url: url.to_string(),
                bytes: stored.len(),
            });
        }
        Some(self.checked_handle(url, sha, &stored, expected_sha))
    }

    /// Downloads one stub body with the downloaded progress event.
    ///
    /// # Errors
    ///
    /// Missing stubs fail as plan errors naming the URL. User
    /// sha mismatches fail as plan errors naming the URL.
    /// Unwritable cache files fail as plan errors naming the
    /// path.
    fn serve_download(
        &self,
        url: &str,
        expected_sha: Option<&Sha>,
        progress: Option<&ProgressSender>,
    ) -> Result<FetchHandle> {
        let found = self.lookup(url);
        let (sha, stored) = match found {
            Some(hit) => hit,
            None => return Err(Error::Plan(format!("no stub for '{url}'"))),
        };
        let mut calls = match self.calls.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let count = calls.get(url).copied().unwrap_or(0);
        calls.insert(url.to_string(), count + 1);
        drop(calls);
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchDownloaded {
                url: url.to_string(),
                bytes: stored.len(),
            });
        }
        self.checked_handle(url, sha, &stored, expected_sha)
    }

    /// Builds the handle for checked bytes under the content path.
    ///
    /// # Errors
    ///
    /// User sha mismatches fail as plan errors naming the URL.
    /// Unwritable cache files fail as plan errors naming the
    /// path.
    fn checked_handle(
        &self,
        url: &str,
        sha: Sha,
        body: &[u8],
        expected_sha: Option<&Sha>,
    ) -> Result<FetchHandle> {
        check_sha(url, body, expected_sha)?;
        self.persist(&sha, body)?;
        FetchHandle::new(self.cache_path(&sha.hex()), sha, url)
    }

    /// Persists cached bytes under the content path.
    ///
    /// Handles promise readable canonical paths.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as plan errors naming
    /// the path.
    fn persist(&self, sha: &Sha, body: &[u8]) -> Result<()> {
        let path = self.cache_path(&sha.hex());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::Plan(format!(
                    "cannot write cache file '{}': {error}",
                    path.display()
                ))
            })?;
        }
        std::fs::write(&path, body).map_err(|error| {
            Error::Plan(format!(
                "cannot write cache file '{}': {error}",
                path.display()
            ))
        })
    }
}

impl FetchCache for MemoryFetchCache {
    fn fetch(
        &self,
        url: &str,
        expected_sha: Option<Sha>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<FetchHandle> {
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchStarted {
                url: url.to_string(),
            });
        }
        if !re_fetch && let Some(hit) = self.serve_hit(url, expected_sha.as_ref(), progress) {
            return hit;
        }
        self.serve_download(url, expected_sha.as_ref(), progress)
    }

    fn read(&self, handle: &FetchHandle) -> Result<Vec<u8>> {
        let guard = match self.bodies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(body) = guard.get(&handle.sha().hex()) {
            return Ok(body.clone());
        }
        drop(guard);
        std::fs::read(handle.canonical()).map_err(|error| {
            Error::Plan(format!(
                "cannot read '{}': {error}",
                handle.canonical().display()
            ))
        })
    }

    fn open(&self, handle: &FetchHandle) -> Result<Box<dyn std::io::Read>> {
        let guard = match self.bodies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(body) = guard.get(&handle.sha().hex()) {
            return Ok(Box::new(std::io::Cursor::new(body.clone())) as Box<dyn std::io::Read>);
        }
        drop(guard);
        std::fs::File::open(handle.canonical())
            .map(|file| Box::new(file) as Box<dyn std::io::Read>)
            .map_err(|error| {
                Error::Plan(format!(
                    "cannot read '{}': {error}",
                    handle.canonical().display()
                ))
            })
    }
}

/// Reports whether one digest holds 64 hex characters.
fn is_hex64(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Checks fetched bytes against the user sha.
///
/// # Errors
///
/// Mismatches fail as plan errors naming the URL.
fn check_sha(url: &str, bytes: &[u8], expected: Option<&Sha>) -> Result<()> {
    let Some(wanted) = expected else {
        return Ok(());
    };
    let actual = Sha::hash(bytes);
    if actual != *wanted {
        return Err(Error::Plan(format!(
            "sha256 mismatch for '{url}': want {wanted}, got {actual}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read as _;
    use std::path::Path;

    const URL: &str = "https://example.com/tool.bin";

    #[test]
    fn single_door_serves_hit_without_redownload() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        match cache.fetch(URL, None, false, None) {
            Ok(hit) => {
                assert_eq!(hit.sha(), &Sha::hash(b"1.2.3"));
                assert_eq!(cache.calls(URL), 0, "hit makes no download");
            }
            Err(error) => panic!("hit serves: {error}"),
        }
        let wanted = Sha::hash(b"1.2.3");
        match cache.fetch(URL, Some(wanted.clone()), false, None) {
            Ok(hit) => assert_eq!(hit.sha(), &wanted),
            Err(error) => panic!("good user sha serves: {error}"),
        }
        match cache.fetch(URL, Some(Sha::new("0".repeat(64)).unwrap()), false, None) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => assert!(error.to_string().contains(URL)),
        }
        match cache.fetch(URL, None, true, None) {
            Ok(refetched) => {
                assert_eq!(refetched.sha(), &wanted);
                assert_eq!(cache.calls(URL), 1, "re-fetch downloads once");
            }
            Err(error) => panic!("re-fetch serves: {error}"),
        }
    }

    #[test]
    fn body_tamper_reads_as_miss_without_serving() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"genuine");
        assert!(cache.lookup(URL).is_some());
        let digest = Sha::hash(b"genuine").hex();
        cache
            .bodies
            .lock()
            .unwrap()
            .insert(digest, b"tampered".to_vec());
        assert!(cache.lookup(URL).is_none());
        match cache.fetch(URL, None, false, None) {
            Ok(handle) => {
                let found = cache.read(&handle).unwrap_or_default();
                assert_ne!(found, b"tampered".to_vec(), "tampered bytes never serve");
                panic!("tampered entry serves");
            }
            Err(error) => assert!(
                error.to_string().contains("no stub"),
                "body tamper misses: {error}"
            ),
        }
    }

    #[test]
    fn corrupt_index_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"genuine");
        assert!(cache.lookup(URL).is_some());
        cache
            .index
            .lock()
            .unwrap()
            .insert(URL.to_string(), "not-a-hex-digest".to_string());
        assert!(cache.lookup(URL).is_none());
        match cache.fetch(URL, None, false, None) {
            Ok(_) => panic!("corrupt index serves"),
            Err(error) => assert!(
                error.to_string().contains("no stub"),
                "corrupt index misses: {error}"
            ),
        }
    }

    #[test]
    fn missing_index_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"genuine");
        assert!(cache.lookup(URL).is_some());
        cache.index.lock().unwrap().remove(URL);
        assert!(cache.lookup(URL).is_none());
        match cache.fetch(URL, None, false, None) {
            Ok(_) => panic!("index-less entry serves"),
            Err(error) => assert!(
                error.to_string().contains("no stub"),
                "index loss misses: {error}"
            ),
        }
    }

    #[test]
    fn user_sha_mismatch_fails_after_hit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        match cache.fetch(URL, Some(Sha::new("0".repeat(64)).unwrap()), false, None) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(URL), "error names the url: {text}");
                assert!(
                    text.contains("sha256 mismatch"),
                    "error reports loss: {text}"
                );
            }
        }
    }

    #[test]
    fn hit_path_stays_jailed_under_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"binary");
        match cache.fetch(URL, None, false, None) {
            Ok(handle) => {
                let path = handle.canonical().to_path_buf();
                assert!(
                    path.starts_with(dir.path().join("cache")),
                    "cache path stays jailed: {}",
                    path.display()
                );
                assert_eq!(
                    path,
                    dir.path().join("cache").join(Sha::hash(b"binary").hex())
                );
            }
            Err(error) => panic!("cached file serves: {error}"),
        }
    }

    #[test]
    fn hit_reports_started_and_cached_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        let (sender, receiver) = crossbeam_channel::unbounded();
        match cache.fetch(URL, None, false, Some(&sender)) {
            Ok(_) => {
                drop(sender);
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
                assert_eq!(cache.calls(URL), 0, "hit makes no download");
            }
            Err(error) => panic!("hit reports progress: {error}"),
        }
    }

    #[test]
    fn refetch_reports_started_and_downloaded_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        let (sender, receiver) = crossbeam_channel::unbounded();
        match cache.fetch(URL, None, true, Some(&sender)) {
            Ok(_) => {
                drop(sender);
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
                assert_eq!(cache.calls(URL), 1, "re-fetch downloads once");
            }
            Err(error) => panic!("re-fetch reports progress: {error}"),
        }
    }

    fn assert_content_path_clean(path: &Path) {
        assert!(
            path.extension().is_none_or(|ext| ext != "sha"),
            "unexpected sha-suffixed file remains: {}",
            path.display()
        );
    }

    #[test]
    fn handles_use_content_address() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        match cache.fetch(URL, None, false, None) {
            Ok(handle) => {
                assert_content_path_clean(handle.canonical());
                assert_eq!(
                    handle.canonical(),
                    &dir.path().join("cache").join(Sha::hash(b"1.2.3").hex())
                );
            }
            Err(error) => panic!("hit serves: {error}"),
        }
    }

    #[test]
    fn open_streams_map_body() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        match cache.fetch(URL, None, false, None) {
            Ok(handle) => match cache.open(&handle) {
                Ok(mut reader) => {
                    let mut found = Vec::new();
                    reader.read_to_end(&mut found).unwrap();
                    assert_eq!(found, b"1.2.3".to_vec());
                }
                Err(error) => panic!("map body opens: {error}"),
            },
            Err(error) => panic!("map body serves: {error}"),
        }
    }

    #[test]
    fn open_streams_large_body() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > 8 * 1024);
        cache.insert(URL, &raw);
        match cache.fetch(URL, None, false, None) {
            Ok(handle) => match cache.open(&handle) {
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
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        let missing = dir.path().join("cache").join("missing-body");
        let handle = FetchHandle::new(missing.clone(), Sha::hash(b"absent"), URL).unwrap();
        match cache.open(&handle) {
            Ok(_) => panic!("missing body opens"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("cannot read"),
                    "missing reads as plan error: {text}"
                );
                assert!(
                    text.contains(&missing.display().to_string()),
                    "error names the cache path: {text}"
                );
            }
        }
    }

    #[test]
    fn read_serves_whole_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"whole view bytes");
        match cache.fetch(URL, None, false, None) {
            Ok(handle) => match cache.read(&handle) {
                Ok(found) => assert_eq!(found, b"whole view bytes".to_vec()),
                Err(error) => panic!("map body reads: {error}"),
            },
            Err(error) => panic!("map body serves: {error}"),
        }
    }
}
