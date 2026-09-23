//! Fetch
//!
//! Content-addressed fetch cache unifying downloads and blobs.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::ids::sha256_hex;
use confit_core::progress::{Event, ProgressSender};

/// Content-addressed fetch cache for remote bytes.
///
/// One folder holds downloads under URL hashes with a
/// `url -> sha` index. Tampered entries read as misses.
/// Offline hits call no fetcher. User shas check fatal
/// after hit-or-download.
pub trait FetchCache {
    /// Fetches one URL body through hit-or-download with sha check.
    ///
    /// # Errors
    ///
    /// Transport failures fail as plan errors. Sha mismatches
    /// fail as plan errors naming the URL.
    fn fetch_bytes(
        &self,
        url: &str,
        expected_sha: Option<&str>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<Vec<u8>>;

    /// Fetches one URL into the cache and returns its path.
    ///
    /// # Errors
    ///
    /// Transport failures fail as plan errors. Sha mismatches
    /// fail as plan errors naming the URL.
    fn fetch_file(
        &self,
        url: &str,
        expected_sha: Option<&str>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf>;
}

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
}

impl FetchCache for MemoryFetchCache {
    fn fetch_bytes(
        &self,
        url: &str,
        expected_sha: Option<&str>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<Vec<u8>> {
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
            return Ok(stored);
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
        Ok(stored)
    }

    fn fetch_file(
        &self,
        url: &str,
        expected_sha: Option<&str>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf> {
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
            return Ok(self.cache_path(url));
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
        Ok(self.cache_path(url))
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
