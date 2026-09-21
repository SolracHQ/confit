//! Fetch
//!
//! Network access behind a trait and cache path helpers.

use std::collections::HashMap;
use std::io::Read as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sha2::Digest as _;

/// Body cap shared by buffered and streamed reads.
const BODY_LIMIT_BYTES: u64 = 1024 * 1024 * 1024;

/// Chunk size for streamed cache writes.
const STREAM_BUF_BYTES: usize = 8 * 1024;

/// Network source for remote bytes.
///
/// # Examples
///
/// ```rust
/// use confit_engine::fetch::MemoryFetch;
/// use confit_engine::fetch::Fetch;
///
/// let fake = MemoryFetch::new();
/// fake.insert("https://example.com/version", b"1");
/// let body = fake.fetch("https://example.com/version");
/// assert!(matches!(body, Ok(body) if body == b"1".to_vec()));
/// ```
pub trait Fetch: Send + Sync + std::fmt::Debug {
    /// Fetches one URL body as bytes.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    ///
    /// # Returns
    ///
    /// The response body bytes.
    ///
    /// # Errors
    ///
    /// Missing stubs and transport failures fail as plan errors.
    ///
    fn fetch(&self, url: &str) -> confit_core::error::Result<Vec<u8>>;

    /// Streams one URL body as a reader.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    ///
    /// # Returns
    ///
    /// The response body reader capped at the shared body limit.
    ///
    /// # Errors
    ///
    /// Missing stubs and transport failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_engine::fetch::{Fetch, MemoryFetch};
    /// use std::io::Read as _;
    ///
    /// let fake = MemoryFetch::new();
    /// fake.insert("https://example.com/version", b"1");
    /// let reader = fake.fetch_stream("https://example.com/version");
    /// let mut body = Vec::new();
    /// match reader {
    ///     Ok(mut reader) => { let _ = reader.read_to_end(&mut body); }
    ///     Err(error) => panic!("stream reads: {error}"),
    /// }
    /// assert_eq!(body, b"1".to_vec());
    /// ```
    fn fetch_stream(&self, url: &str) -> confit_core::error::Result<Box<dyn std::io::Read>>;
}

/// Blocking HTTP source for remote bytes.
///
#[derive(Debug, Clone, Copy, Default)]
pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn fetch(&self, url: &str) -> confit_core::error::Result<Vec<u8>> {
        let mut response = ureq::get(url)
            .call()
            .map_err(|error| crate::error::plan(format!("cannot fetch '{url}': {error}")))?;
        response
            .body_mut()
            .with_config()
            .limit(BODY_LIMIT_BYTES)
            .read_to_vec()
            .map_err(|error| crate::error::plan(format!("cannot fetch '{url}': {error}")))
    }

    fn fetch_stream(&self, url: &str) -> confit_core::error::Result<Box<dyn std::io::Read>> {
        let response = ureq::get(url)
            .call()
            .map_err(|error| crate::error::plan(format!("cannot fetch '{url}': {error}")))?;
        let reader = response
            .into_body()
            .into_with_config()
            .limit(BODY_LIMIT_BYTES)
            .reader();
        Ok(Box::new(reader) as Box<dyn std::io::Read>)
    }
}

/// Memory source keyed by URL for tests.
///
#[derive(Debug, Default)]
pub struct MemoryFetch {
    /// Bodies and call counts behind one lock.
    inner: Mutex<MemoryInner>,
}

/// Bodies and call counts for the memory source.
#[derive(Debug, Default)]
struct MemoryInner {
    /// Bodies keyed by URL.
    bodies: HashMap<String, Vec<u8>>,
    /// Call counts keyed by URL.
    calls: HashMap<String, usize>,
}

impl MemoryFetch {
    /// Builds one empty memory source.
    ///
    /// # Returns
    ///
    /// Empty source with zero stubs.
    ///
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(MemoryInner::default()),
        }
    }

    /// Stores one URL body stub.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    /// * `body` - the stubbed response bytes.
    ///
    /// # Returns
    ///
    /// Unit once the stub lands.
    ///
    pub fn insert(&self, url: &str, body: &[u8]) {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.bodies.insert(url.to_string(), body.to_vec());
    }

    /// Reads call counts for one URL.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    ///
    /// # Returns
    ///
    /// The fetch count, zero for unseen URLs.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_engine::fetch::{Fetch, MemoryFetch};
    ///
    /// let fake = MemoryFetch::new();
    /// fake.insert("https://example.com/version", b"1");
    /// assert!(matches!(fake.fetch("https://example.com/version"), Ok(_)));
    /// assert_eq!(fake.calls("https://example.com/version"), 1);
    /// ```
    pub fn calls(&self, url: &str) -> usize {
        let guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.calls.get(url).copied().unwrap_or(0)
    }
}

impl Fetch for MemoryFetch {
    fn fetch(&self, url: &str) -> confit_core::error::Result<Vec<u8>> {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let count = guard.calls.get(url).copied().unwrap_or(0);
        guard.calls.insert(url.to_string(), count + 1);
        match guard.bodies.get(url) {
            Some(body) => Ok(body.clone()),
            None => Err(crate::error::plan(format!("no stub for '{url}'"))),
        }
    }

    fn fetch_stream(&self, url: &str) -> confit_core::error::Result<Box<dyn std::io::Read>> {
        let mut guard = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let count = guard.calls.get(url).copied().unwrap_or(0);
        guard.calls.insert(url.to_string(), count + 1);
        match guard.bodies.get(url) {
            Some(body) => {
                Ok(Box::new(std::io::Cursor::new(body.clone())) as Box<dyn std::io::Read>)
            }
            None => Err(crate::error::plan(format!("no stub for '{url}'"))),
        }
    }
}

/// Owner for cached remote bytes on disk.
///
#[derive(Debug, Clone)]
pub struct Cache {
    /// Cache folder holding hashed bodies and sidecars.
    dir: PathBuf,
}

impl Cache {
    /// Builds one cache owner holding the cache folder.
    ///
    /// # Arguments
    ///
    /// * `dir` - the cache folder.
    ///
    /// # Returns
    ///
    /// Owner holding the folder path.
    ///
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Reads cached bytes passing the sidecar digest check.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    ///
    /// # Returns
    ///
    /// The cached bytes, holding `None` for miss and mismatch.
    ///
    pub fn lookup(&self, url: &str) -> Option<Vec<u8>> {
        let cached = cache_path(&self.dir, url);
        let sidecar = sidecar_path(&cached);
        let stored = std::fs::read(&cached).ok()?;
        let stored_sha = std::fs::read_to_string(&sidecar).ok()?;
        if confit_core::ids::sha256_hex(&stored) == stored_sha.trim().to_lowercase() {
            Some(stored)
        } else {
            None
        }
    }

    /// Writes bytes and the sidecar digest for one URL.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    /// * `bytes` - the response body bytes.
    ///
    /// # Returns
    ///
    /// The cache file path holding the bytes.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    ///
    pub fn store(&self, url: &str, bytes: &[u8]) -> std::io::Result<PathBuf> {
        let cached = cache_path(&self.dir, url);
        let sidecar = sidecar_path(&cached);
        if let Some(parent) = cached.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&cached, bytes)?;
        let digest = confit_core::ids::sha256_hex(bytes);
        std::fs::write(&sidecar, digest.as_bytes())?;
        Ok(cached)
    }

    /// Writes a streamed body and the sidecar digest for one URL.
    ///
    /// # Arguments
    ///
    /// * `url` - the remote address.
    /// * `reader` - the response body reader.
    ///
    /// # Returns
    ///
    /// The cache file path holding the streamed bytes.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors. Read failures
    /// on the body reader fail as io errors.
    ///
    pub fn store_stream(&self, url: &str, reader: impl std::io::Read) -> std::io::Result<PathBuf> {
        let cached = cache_path(&self.dir, url);
        let sidecar = sidecar_path(&cached);
        if let Some(parent) = cached.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = std::fs::File::create(&cached)?;
        let mut writer = std::io::BufWriter::new(file);
        let mut limited = reader.take(BODY_LIMIT_BYTES);
        let mut hasher = sha2::Sha256::new();
        let mut buf = [0u8; STREAM_BUF_BYTES];
        loop {
            let read = limited.read(&mut buf)?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            writer.write_all(&buf[..read])?;
        }
        writer.flush()?;
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        std::fs::write(&sidecar, digest.as_bytes())?;
        Ok(cached)
    }
}

/// Derives the cache file path for one URL.
///
/// # Arguments
///
/// * `cache` - the cache folder.
/// * `url` - the remote address.
///
/// # Returns
///
/// Absolute cache file path for the URL.
///
pub fn cache_path(cache: &Path, url: &str) -> PathBuf {
    cache.join(confit_core::ids::sha256_hex(url.as_bytes()))
}

/// Derives the sidecar path beside one cached file.
///
/// # Arguments
///
/// * `cached` - the cached file path.
///
/// # Returns
///
/// The sidecar path holding the hex digest.
///
/// # Examples
///
/// ```rust
/// use confit_engine::fetch::sidecar_path;
/// use std::path::Path;
///
/// let cached = Path::new("/cache/abc");
/// let sidecar = sidecar_path(cached);
/// assert_eq!(sidecar.parent(), cached.parent());
/// ```
pub fn sidecar_path(cached: &Path) -> PathBuf {
    let mut text = cached.as_os_str().to_owned();
    text.push(".sha");
    PathBuf::from(text)
}

/// Resolves the cache folder with test override.
///
/// # Arguments
///
/// * `override_dir` - the test override, holding `None` for the OS cache.
///
/// # Returns
///
/// The override, else the OS cache folder joined with confit.
///
/// # Errors
///
/// Missing OS cache folders fail as plan errors.
///
pub fn resolve_cache_dir(override_dir: Option<&Path>) -> confit_core::error::Result<PathBuf> {
    if let Some(dir) = override_dir {
        return Ok(dir.to_path_buf());
    }
    match dirs::cache_dir() {
        Some(dir) => Ok(dir.join("confit")),
        None => Err(crate::error::plan(
            "cannot resolve cache directory".to_string(),
        )),
    }
}
