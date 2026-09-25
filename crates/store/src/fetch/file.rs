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

/// Index folder holding url-hash to content-hash entries.
const INDEX_DIR: &str = "urls";

/// Staging suffix for atomic cache writes.
const STAGING_SUFFIX: &str = ".part";

/// File-backed fetch cache for remote bytes.
///
/// Bodies ride `{cache}/{content-sha}` behind a `urls` index.
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

    /// Derives the content path for one content digest.
    fn content_path(&self, sha_hex: &str) -> PathBuf {
        self.cache.join(sha_hex)
    }

    /// Derives the index entry path for one URL.
    fn index_path(&self, url: &str) -> PathBuf {
        self.cache
            .join(INDEX_DIR)
            .join(Sha::hash(url.as_bytes()).hex())
    }

    /// Derives the staging path holding one URL download.
    ///
    /// The staging file carries the `.part` suffix until rename.
    fn staging_path(&self, url: &str) -> PathBuf {
        let mut text = self
            .cache
            .join(Sha::hash(url.as_bytes()).hex())
            .into_os_string();
        text.push(STAGING_SUFFIX);
        PathBuf::from(text)
    }

    /// Reads a cached handle passing the index digest check.
    ///
    /// Tampered entries read as misses.
    fn lookup(&self, url: &str) -> Option<FetchHandle> {
        let stored = std::fs::read_to_string(self.index_path(url)).ok()?;
        let wanted = Sha::new(stored.trim().to_lowercase()).ok()?;
        let content = self.content_path(&wanted.hex());
        let mut file = std::fs::File::open(&content).ok()?;
        let actual = Sha::read(&mut file).ok()?;
        if actual != wanted {
            return None;
        }
        FetchHandle::new(content, actual, url).ok()
    }

    /// Serves one cached hit with the cached progress event.
    ///
    /// Misses answer none so the caller downloads.
    ///
    /// # Errors
    ///
    /// Cached hits failing the user sha fail as plan errors
    /// naming the URL.
    fn fetch_hit(
        &self,
        url: &str,
        expected_sha: Option<&Sha>,
        progress: Option<&ProgressSender>,
    ) -> Option<Result<FetchHandle>> {
        let hit = self.lookup(url)?;
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchCached {
                url: url.to_string(),
                bytes: file_len(hit.canonical()),
            });
        }
        match check_sha(url, hit.sha(), expected_sha) {
            Ok(()) => Some(Ok(hit)),
            Err(error) => Some(Err(error)),
        }
    }

    /// Downloads one URL body with the downloaded progress event.
    ///
    /// # Errors
    ///
    /// Transport failures fail as plan errors naming the URL.
    /// Sha mismatches fail as plan errors naming the URL.
    fn fetch_download(
        &self,
        url: &str,
        expected_sha: Option<&Sha>,
        progress: Option<&ProgressSender>,
    ) -> Result<FetchHandle> {
        let (path, sha, bytes) = self.download(url)?;
        if let Some(sender) = progress {
            let _ = sender.send(Event::FetchDownloaded {
                url: url.to_string(),
                bytes,
            });
        }
        check_sha(url, &sha, expected_sha)?;
        FetchHandle::new(path, sha, url)
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

    /// Writes a streamed body with the index entry for one URL.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as plan errors naming
    /// the URL. Read failures on the body reader fail as plan
    /// errors naming the URL.
    fn store_stream(&self, url: &str, reader: impl std::io::Read) -> Result<(PathBuf, Sha, usize)> {
        let staging = self.staging_path(url);
        if let Some(parent) = staging.parent() {
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
        let content = self.content_path(&digest.hex());
        if let Err(error) = std::fs::rename(&staging, &content) {
            let _ = std::fs::remove_file(&staging);
            return Err(Error::Plan(format!(
                "cannot write cache for '{url}': {error}"
            )));
        }
        self.write_index(url, &digest)?;
        Ok((content, digest, bytes))
    }

    /// Writes the url index entry for one content digest.
    ///
    /// # Errors
    ///
    /// Unwritable index folders and files fail as plan errors
    /// naming the URL.
    fn write_index(&self, url: &str, digest: &Sha) -> Result<()> {
        let index = self.index_path(url);
        if let Some(parent) = index.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))?;
        }
        std::fs::write(&index, digest.hex().as_bytes())
            .map_err(|error| Error::Plan(format!("cannot write cache for '{url}': {error}")))
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
        if !re_fetch && let Some(hit) = self.fetch_hit(url, expected_sha.as_ref(), progress) {
            return hit;
        }
        self.fetch_download(url, expected_sha.as_ref(), progress)
    }

    fn read(&self, handle: &FetchHandle) -> Result<Vec<u8>> {
        std::fs::read(handle.canonical()).map_err(|error| {
            Error::Plan(format!(
                "cannot read '{}': {error}",
                handle.canonical().display()
            ))
        })
    }

    fn open(&self, handle: &FetchHandle) -> Result<Box<dyn std::io::Read>> {
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

/// Reads one file length without loading content.
///
/// Missing files read zero for event sizes alone.
fn file_len(path: &Path) -> usize {
    match std::fs::metadata(path) {
        Ok(facts) => usize::try_from(facts.len()).unwrap_or(0),
        Err(_) => 0,
    }
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

    fn index_file(dir: &Path, url: &str) -> PathBuf {
        dir.join("cache")
            .join(INDEX_DIR)
            .join(Sha::hash(url.as_bytes()).hex())
    }

    fn content_file(dir: &Path, sha_hex: &str) -> PathBuf {
        dir.join("cache").join(sha_hex)
    }

    fn seed(dir: &Path, url: &str, body: &[u8]) -> PathBuf {
        let digest = Sha::hash(body);
        let content = content_file(dir, &digest.hex());
        if let Some(parent) = content.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&content, body).unwrap();
        let index = index_file(dir, url);
        if let Some(parent) = index.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&index, digest.hex()).unwrap();
        content
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
        let content = seed(dir.path(), DARK_URL, b"genuine");
        assert!(cache.lookup(DARK_URL).is_some());
        std::fs::write(&content, b"tampered").unwrap();
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
    fn missing_index_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"genuine");
        std::fs::remove_file(index_file(dir.path(), DARK_URL)).unwrap();
        assert!(cache.lookup(DARK_URL).is_none());
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(_) => panic!("index-less entry serves"),
            Err(error) => assert!(
                error.to_string().contains("cannot fetch"),
                "index loss redownloads: {error}"
            ),
        }
    }

    #[test]
    fn corrupt_index_reads_as_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"genuine");
        assert!(cache.lookup(DARK_URL).is_some());
        std::fs::write(index_file(dir.path(), DARK_URL), b"not-a-hex-digest").unwrap();
        assert!(cache.lookup(DARK_URL).is_none());
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(_) => panic!("corrupt index serves"),
            Err(error) => assert!(
                error.to_string().contains("cannot fetch"),
                "corrupt index redownloads: {error}"
            ),
        }
    }

    #[test]
    fn layout_uses_content_address_with_url_index() {
        let dir = tempfile::tempdir().unwrap();
        seed(dir.path(), DARK_URL, b"1.2.3");
        let wanted = Sha::hash(b"1.2.3");
        assert_eq!(
            content_file(dir.path(), &wanted.hex()),
            dir.path().join("cache").join(wanted.hex())
        );
        assert!(content_file(dir.path(), &wanted.hex()).exists());
        let index = index_file(dir.path(), DARK_URL);
        assert_eq!(
            index,
            dir.path()
                .join("cache")
                .join(INDEX_DIR)
                .join(Sha::hash(DARK_URL.as_bytes()).hex())
        );
        assert_eq!(std::fs::read_to_string(&index).unwrap(), wanted.hex());
        assert_no_sha_suffix_files(&dir.path().join("cache"));
    }

    #[test]
    fn hit_reports_started_and_cached_with_fields() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"1.2.3");
        let (sender, receiver) = crossbeam_channel::unbounded();
        match cache.fetch(DARK_URL, None, false, Some(&sender)) {
            Ok(_) => {
                drop(sender);
                let events: Vec<Event> = receiver.try_iter().collect();
                assert_eq!(
                    events,
                    vec![
                        Event::FetchStarted {
                            url: DARK_URL.to_string(),
                        },
                        Event::FetchCached {
                            url: DARK_URL.to_string(),
                            bytes: 5,
                        },
                    ]
                );
            }
            Err(error) => panic!("hit reports progress: {error}"),
        }
    }

    fn assert_no_sha_suffix_files(dir: &Path) {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(next) = stack.pop() {
            for entry in std::fs::read_dir(&next).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
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

    #[test]
    fn open_streams_seeded_body() {
        let dir = tempfile::tempdir().unwrap();
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"1.2.3");
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => match cache.open(&handle) {
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
        let cache = file_cache(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > STREAM_BUF_BYTES);
        seed(dir.path(), DARK_URL, &raw);
        match cache.fetch(DARK_URL, None, false, None) {
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
        let cache = file_cache(dir.path());
        let missing = dir.path().join("cache").join("missing-body");
        let handle = FetchHandle::new(missing.clone(), Sha::hash(b"absent"), DARK_URL).unwrap();
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
        let cache = file_cache(dir.path());
        seed(dir.path(), DARK_URL, b"whole view bytes");
        match cache.fetch(DARK_URL, None, false, None) {
            Ok(handle) => match cache.read(&handle) {
                Ok(found) => assert_eq!(found, b"whole view bytes".to_vec()),
                Err(error) => panic!("seeded body reads: {error}"),
            },
            Err(error) => panic!("seeded body serves: {error}"),
        }
    }

    #[test]
    fn pooled_paths_agree_for_identical_content() {
        use crate::blob::{BlobStore, file::FileBlobStore};

        let dir = tempfile::tempdir().unwrap();
        let roots = StoreRoots {
            config_base: dir.path().join("config"),
            ..Default::default()
        };
        let store = FileBlobStore::new(&roots);
        let small = b"shared pool bytes".to_vec();
        let via_put = store.put(&small).unwrap();
        let mut reader = std::io::Cursor::new(small.clone());
        let via_reader = store.put_reader(&mut reader).unwrap();
        let source_path = dir.path().join("source.bin");
        std::fs::write(&source_path, &small).unwrap();
        let source = FetchHandle::new(source_path, Sha::hash(&small), DARK_URL).unwrap();
        let via_source = store.put_source(&source).unwrap();
        assert_eq!(via_reader, via_put, "reader path keeps sealed identity");
        assert_eq!(via_source, via_put, "trusted source keeps sealed identity");
        let large: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(large.len() > STREAM_BUF_BYTES);
        let large_put = store.put(&large).unwrap();
        let mut large_reader = std::io::Cursor::new(large.clone());
        let large_streamed = store.put_reader(&mut large_reader).unwrap();
        let large_path = dir.path().join("large.bin");
        std::fs::write(&large_path, &large).unwrap();
        let large_source = FetchHandle::new(large_path, Sha::hash(&large), DARK_URL).unwrap();
        let large_sourced = store.put_source(&large_source).unwrap();
        assert_eq!(
            large_streamed.sha(),
            large_put.sha(),
            "streamed bytes keep content hash"
        );
        assert_eq!(
            large_sourced.sha(),
            large_put.sha(),
            "sourced bytes keep content hash"
        );
        assert_eq!(
            large_streamed, large_sourced,
            "streaming paths keep one sealed identity"
        );
        for handle in [&large_put, &large_streamed, &large_sourced] {
            let mut found = Vec::new();
            store.open(handle).unwrap().read_to_end(&mut found).unwrap();
            assert_eq!(found, large, "pooled bytes round-trip");
        }
    }
}
