//! File
//!
//! File-backed fetch cache for remote bytes.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use confit_core::error::{Error, Result};
use confit_core::handles::{FetchHandle, Sha, TrustedHandle};
use confit_core::progress::{Event, ProgressSender};
use sha2::Digest as _;

use super::FetchCache;
use crate::StoreRoots;

/// Body cap shared by buffered and streamed reads.
const BODY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

/// Chunk size for streamed cache writes.
const STREAM_BUF_BYTES: usize = 8 * 1024;

/// Sidecar suffix holding the body digest.
const SIDECAR_SUFFIX: &str = ".sha";

/// Staging suffix for atomic cache writes.
const STAGING_SUFFIX: &str = ".part";

/// File-backed fetch cache for remote bytes.
///
/// Entries ride `{cache}/{url-sha}` with `.sha` sidecars.
#[derive(Debug, Clone)]
pub struct FileFetchCache {
    cache: PathBuf,
}

impl FileFetchCache {
    /// Builds one file-backed fetch cache under the cache base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            cache: roots.cache_base.clone(),
        }
    }

    /// Derives the cache file path for one URL.
    fn cache_path(&self, url: &str) -> PathBuf {
        self.cache.join(Sha::hash(url.as_bytes()).hex())
    }

    /// Reads a cached handle passing the sidecar digest check.
    ///
    /// Tampered entries read as misses.
    fn lookup(&self, url: &str) -> Option<FetchHandle> {
        let cached = self.cache_path(url);
        let stored_sha = std::fs::read_to_string(sidecar_path(&cached)).ok()?;
        let wanted = stored_sha.trim().to_lowercase();
        if !is_hex64(&wanted) {
            return None;
        }
        let mut file = std::fs::File::open(&cached).ok()?;
        let actual = Sha::read(&mut file).ok()?;
        if actual != Sha::new(wanted).ok()? {
            return None;
        }
        FetchHandle::new(cached, actual, url).ok()
    }

    /// Downloads one URL body into the cache through staging.
    ///
    /// # Errors
    ///
    /// Transport failures fail as plan errors naming the URL.
    fn download(&self, url: &str) -> Result<(PathBuf, Sha, usize)> {
        let response = ureq::get(url)
            .call()
            .map_err(|error| Error::Plan(format!("cannot fetch '{url}': {error}")))?;
        let reader = response
            .into_body()
            .into_with_config()
            .limit(BODY_LIMIT_BYTES)
            .reader();
        self.store_stream(url, reader)
    }

    /// Writes a streamed body with the sidecar digest for one URL.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as plan errors naming
    /// the URL. Read failures on the body reader fail as plan
    /// errors naming the URL.
    fn store_stream(&self, url: &str, reader: impl std::io::Read) -> Result<(PathBuf, Sha, usize)> {
        let cached = self.cache_path(url);
        let staging = staging_path(&cached);
        let sidecar = sidecar_path(&cached);
        if let Some(parent) = cached.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
        }
        let outcome = self.stream_to_staging(url, reader, &staging);
        let (digest, bytes) = match outcome {
            Ok(done) => done,
            Err(error) => {
                let _ = std::fs::remove_file(&staging);
                return Err(error);
            }
        };
        if let Err(error) = std::fs::rename(&staging, &cached) {
            let _ = std::fs::remove_file(&staging);
            return Err(Error::Plan(format!(
                "cannot write cache for '{url}': {error}"
            )));
        }
        std::fs::write(&sidecar, digest.hex().as_bytes())
            .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
        Ok((cached, digest, bytes))
    }

    /// Streams one body into staging while hashing.
    ///
    /// # Errors
    ///
    /// Unwritable staging files fail as plan errors naming
    /// the URL. Read failures on the body reader fail as plan
    /// errors naming the URL.
    fn stream_to_staging(
        &self,
        url: &str,
        reader: impl std::io::Read,
        staging: &Path,
    ) -> Result<(Sha, usize)> {
        let file = std::fs::File::create(staging)
            .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
        let mut writer = std::io::BufWriter::new(file);
        let mut limited = reader.take(BODY_LIMIT_BYTES);
        let mut hasher = sha2::Sha256::new();
        let mut buf = [0u8; STREAM_BUF_BYTES];
        let mut bytes: u64 = 0;
        loop {
            let read = limited
                .read(&mut buf)
                .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
            if read == 0 {
                break;
            }
            bytes += read as u64;
            hasher.update(&buf[..read]);
            writer
                .write_all(&buf[..read])
                .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
        }
        writer
            .flush()
            .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
        let digest = Sha::finish(hasher);
        Ok((digest, usize::try_from(bytes).unwrap_or(0)))
    }
}

impl FetchCache for FileFetchCache {
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
        if !re_fetch && let Some(hit) = self.lookup(url) {
            if let Some(sender) = progress {
                let _ = sender.send(Event::FetchCached {
                    url: url.to_string(),
                    bytes: file_len(hit.canonical()),
                });
            }
            check_sha(url, hit.sha(), expected_sha.as_ref())?;
            return Ok(hit);
        }
        let (path, sha, bytes) = self.download(url)?;
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchDownloaded {
                url: url.to_string(),
                bytes,
            });
        }
        check_sha(url, &sha, expected_sha.as_ref())?;
        FetchHandle::new(path, sha, url)
    }

    fn read(&self, handle: &FetchHandle) -> Result<Vec<u8>> {
        std::fs::read(handle.canonical()).map_err(|error| {
            Error::Plan(format!(
                "cannot read '{}': {error}",
                handle.canonical().display()
            ))
        })
    }
}

/// Derives the sidecar path beside one cached file.
///
/// The sidecar holds the body hex digest.
fn sidecar_path(cached: &Path) -> PathBuf {
    let mut text = cached.as_os_str().to_owned();
    text.push(SIDECAR_SUFFIX);
    PathBuf::from(text)
}

/// Derives the staging path beside one cached file.
///
/// The staging file carries the `.part` suffix until rename.
fn staging_path(cached: &Path) -> PathBuf {
    let mut text = cached.as_os_str().to_owned();
    text.push(STAGING_SUFFIX);
    PathBuf::from(text)
}

/// Reads one file length without loading content.
///
/// Missing files read zero for event sizes alone.
fn file_len(path: &Path) -> usize {
    match std::fs::metadata(path) {
        Ok(facts) => usize::try_from(facts.len()).unwrap_or(0),
        Err(_) => 0,
    }
}

/// Reports whether one digest holds 64 hex characters.
fn is_hex64(sha: &str) -> bool {
    sha.len() == 64 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Checks a fetched digest against the user sha.
///
/// # Errors
///
/// Mismatches fail as plan errors naming the URL.
fn check_sha(url: &str, actual: &Sha, expected: Option<&Sha>) -> Result<()> {
    let Some(wanted) = expected else {
        return Ok(());
    };
    if actual != wanted {
        return Err(Error::Plan(format!(
            "sha256 mismatch for '{url}': want {wanted}, got {actual}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const DARK_URL: &str = "http://127.0.0.1:9/seeded";
    const DARK_FILE_URL: &str = "http://127.0.0.1:9/tool.bin";

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            cache_base: dir.join("cache"),
            ..Default::default()
        }
    }

    fn file_cache(dir: &Path) -> FileFetchCache {
        FileFetchCache::new(&test_roots(dir))
    }

    fn cache_file(dir: &Path, url: &str) -> PathBuf {
        dir.join("cache").join(Sha::hash(url.as_bytes()).hex())
    }

    fn seed(dir: &Path, url: &str, body: &[u8]) -> PathBuf {
        let cached = cache_file(dir, url);
        if let Some(parent) = cached.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&cached, body).unwrap();
        let mut sidecar = cached.as_os_str().to_owned();
        sidecar.push(SIDECAR_SUFFIX);
        std::fs::write(PathBuf::from(sidecar), Sha::hash(body).hex()).unwrap();
        cached
    }

    #[test]
    fn bytes_hit_serves_seeded_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        let seeded = seed(dir.path(), DARK_URL, b"1.2.3");
        assert!(seeded.exists());
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => {
                let found = std::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("seeded bytes serve: {error}"),
        }
    }

    #[test]
    fn bytes_hit_makes_no_download() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"1.2.3");
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => {
                let found = std::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("offline hit serves: {error}"),
        }
    }

    #[test]
    fn file_hit_returns_path_under_cache() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_FILE_URL, b"binary");
        match cache.fetch(DARK_FILE_URL, None, false, None) {
            Ok(handle) => {
                let path = handle.canonical().to_path_buf();
                assert!(
                    path.starts_with(dir.path().join("cache")),
                    "cache path stays jailed: {}",
                    path.display()
                );
                match std::fs::read(&path) {
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
        let cache = file_cache(dir.path());
        let cached = seed(dir.path(), DARK_URL, b"genuine");
        assert!(cache.lookup(DARK_URL).is_some());
        std::fs::write(&cached, b"tampered").unwrap();
        assert!(cache.lookup(DARK_URL).is_none());
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => {
                let found = std::fs::read(handle.canonical()).unwrap();
                assert_ne!(found, b"tampered".to_vec(), "tampered bytes never serve");
            }
            Err(error) => assert!(
                error.to_string().contains("cannot fetch"),
                "tamper redownloads: {error}"
            ),
        }
    }

    #[test]
    fn missing_sidecar_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        let cached = seed(dir.path(), DARK_URL, b"genuine");
        let mut sidecar = cached.as_os_str().to_owned();
        sidecar.push(SIDECAR_SUFFIX);
        std::fs::remove_file(PathBuf::from(sidecar)).unwrap();
        assert!(cache.lookup(DARK_URL).is_none());
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(_) => panic!("sidecar-less entry serves"),
            Err(error) => assert!(
                error.to_string().contains("cannot fetch"),
                "sidecar loss redownloads: {error}"
            ),
        }
    }

    #[test]
    fn user_sha_mismatch_fails_after_hit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"1.2.3");
        seed(dir.path(), DARK_FILE_URL, b"binary");
        let wrong = "0".repeat(64);
        match cache.fetch(
            DARK_URL,
            Some(Sha::new(wrong.clone()).unwrap()),
            false,
            None,
        ) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(DARK_URL), "error names the url: {text}");
                assert!(
                    text.contains("sha256 mismatch"),
                    "error reports loss: {text}"
                );
            }
        }
        match cache.fetch(
            DARK_FILE_URL,
            Some(Sha::new(wrong.clone()).unwrap()),
            false,
            None,
        ) {
            Ok(_) => panic!("bad user sha passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(DARK_FILE_URL), "error names the url: {text}");
                assert!(
                    text.contains("sha256 mismatch"),
                    "error reports loss: {text}"
                );
            }
        }
    }

    #[test]
    fn user_sha_match_passes_after_hit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        match cache.fetch(DARK_URL, Some(wanted.clone()), false, None) {
            Ok(handle) => {
                let found = std::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("good user sha serves: {error}"),
        }
    }

    #[test]
    fn re_fetch_forces_download_past_valid_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"1.2.3");
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => {
                let found = std::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, b"1.2.3".to_vec());
            }
            Err(error) => panic!("cached read serves: {error}"),
        }
        match cache.fetch(DARK_URL, None, true, None) {
            Ok(_) => panic!("forced fetch serves the cache"),
            Err(error) => assert!(
                error.to_string().contains("cannot fetch"),
                "forced fetch downloads: {error}"
            ),
        }
    }

    #[test]
    fn bytes_hit_streams_large_entry() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > STREAM_BUF_BYTES);
        seed(dir.path(), DARK_URL, &raw);
        assert!(cache.lookup(DARK_URL).is_some());
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => {
                assert_eq!(handle.sha(), &Sha::hash(&raw));
                let found = std::fs::read(handle.canonical()).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("large hit serves: {error}"),
        }
    }
}
