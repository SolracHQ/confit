//! Memory
//!
//! Memory fetch cache for tests.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::handles::{FetchHandle, TrustedHandle};
use confit_core::ids::sha256_hex;
use confit_core::progress::{Event, ProgressSender};

use super::FetchCache;

/// Memory fetch cache for tests.
///
/// Bodies ride a `url -> bytes` map with call counts proving
/// offline hits.
#[derive(Debug, Default)]
pub struct MemoryFetchCache {
    base: PathBuf,
    bodies: Mutex<HashMap<String, Vec<u8>>>,
    calls: Mutex<HashMap<String, usize>>,
}

impl MemoryFetchCache {
    /// Builds one memory fetch cache holding the cache base.
    pub fn new(base: PathBuf) -> Self {
        Self {
            base,
            bodies: Mutex::new(HashMap::new()),
            calls: Mutex::new(HashMap::new()),
        }
    }

    /// Seeds one URL body stub.
    pub fn insert(&self, url: &str, body: &[u8]) {
        let mut guard = match self.bodies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(url.to_string(), body.to_vec());
    }

    /// Reads download counts for one URL.
    pub fn calls(&self, url: &str) -> usize {
        let guard = match self.calls.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.get(url).copied().unwrap_or(0)
    }

    /// Derives the cache file path for one URL.
    fn cache_path(&self, url: &str) -> PathBuf {
        self.base.join(sha256_hex(url.as_bytes()))
    }

    /// Persists stub bytes under the cache path.
    ///
    /// Handles promise readable canonical paths.
    fn persist(&self, url: &str, body: &[u8]) -> Result<()> {
        let path = self.cache_path(url);
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
        expected_sha: Option<&str>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<FetchHandle> {
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchStarted {
                url: url.to_string(),
            });
        }
        let guard = match self.bodies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let stored = match guard.get(url) {
            Some(stored) => stored.clone(),
            None => return Err(Error::Plan(format!("no stub for '{url}'"))),
        };
        drop(guard);
        if !re_fetch {
            if let Some(sender) = progress {
                let _ = sender.send(Event::FetchCached {
                    url: url.to_string(),
                    bytes: stored.len(),
                });
            }
            check_sha(url, &stored, expected_sha)?;
            self.persist(url, &stored)?;
            return FetchHandle::new(self.cache_path(url), sha256_hex(&stored), url);
        }
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
        check_sha(url, &stored, expected_sha)?;
        self.persist(url, &stored)?;
        FetchHandle::new(self.cache_path(url), sha256_hex(&stored), url)
    }

    fn read(&self, handle: &FetchHandle) -> Result<Vec<u8>> {
        let guard = match self.bodies.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        for body in guard.values() {
            if sha256_hex(body) == handle.sha() {
                return Ok(body.clone());
            }
        }
        drop(guard);
        std::fs::read(handle.canonical()).map_err(|error| {
            Error::Plan(format!(
                "cannot read '{}': {error}",
                handle.canonical().display()
            ))
        })
    }
}

/// Checks fetched bytes against the user sha.
fn check_sha(url: &str, bytes: &[u8], expected: Option<&str>) -> Result<()> {
    let Some(wanted) = expected else {
        return Ok(());
    };
    let actual = sha256_hex(bytes);
    if actual != wanted.to_lowercase() {
        return Err(Error::Plan(format!(
            "sha256 mismatch for '{url}': want {wanted}, got {actual}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://example.com/tool.bin";

    #[test]
    fn single_door_serves_hit_without_redownload() {
        let dir = tempfile::tempdir().unwrap();
        let cache = MemoryFetchCache::new(dir.path().join("cache"));
        cache.insert(URL, b"1.2.3");
        match cache.fetch(URL, None, false, None) {
            Ok(hit) => {
                assert_eq!(hit.sha(), sha256_hex(b"1.2.3"));
                assert_eq!(cache.calls(URL), 0, "hit makes no download");
            }
            Err(error) => panic!("hit serves: {error}"),
        }
        let wanted = sha256_hex(b"1.2.3");
        match cache.fetch(URL, Some(&wanted), false, None) {
            Ok(hit) => assert_eq!(hit.sha(), wanted),
            Err(error) => panic!("good user sha serves: {error}"),
        }
        match cache.fetch(URL, Some(&"0".repeat(64)), false, None) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => assert!(error.to_string().contains(URL)),
        }
        match cache.fetch(URL, None, true, None) {
            Ok(refetched) => {
                assert_eq!(refetched.sha(), wanted);
                assert_eq!(cache.calls(URL), 1, "re-fetch downloads once");
            }
            Err(error) => panic!("re-fetch serves: {error}"),
        }
    }
}
