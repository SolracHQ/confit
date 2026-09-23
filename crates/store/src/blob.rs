//! Blob
//!
//! Content-addressed blob pool behind content hashes.

use std::collections::HashMap;
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::ids::sha256_hex;

/// Content-addressed blob pool.
///
/// Keys hold lowercase hex SHA-256 over raw bytes.
pub trait BlobStore {
    /// Reads raw bytes for one content hash.
    ///
    /// # Errors
    ///
    /// Unknown hashes fail as plan errors naming the hash.
    fn open(&self, sha: &str) -> Result<Vec<u8>>;

    /// Stores raw bytes under their content hash.
    ///
    /// # Errors
    ///
    /// Pool write failures surface as plan or io errors.
    fn put(&self, bytes: &[u8]) -> Result<String>;

    /// Reports whether one content hash reads present.
    fn has(&self, sha: &str) -> bool;

    /// Reads the raw byte count for one content hash.
    ///
    /// # Errors
    ///
    /// Unknown hashes fail as plan errors naming the hash.
    fn len(&self, sha: &str) -> Result<u64>;

    /// Drops pool blobs unreferenced by slots and history.
    ///
    /// # Errors
    ///
    /// Listing and removal failures surface as plan or io errors.
    fn prune(&self) -> Result<usize>;
}

/// Memory blob pool for tests.
///
/// Blobs ride a `hash -> bytes` map behind one lock.
#[derive(Debug, Default)]
pub struct MemoryBlobStore {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryBlobStore {
    /// Builds an empty memory blob pool.
    pub fn new() -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
        }
    }
}

impl BlobStore for MemoryBlobStore {
    fn open(&self, sha: &str) -> Result<Vec<u8>> {
        let guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(sha) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(Error::Plan(format!("missing blob '{sha}'"))),
        }
    }

    fn put(&self, bytes: &[u8]) -> Result<String> {
        let sha = sha256_hex(bytes);
        let mut guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(sha.clone(), bytes.to_vec());
        Ok(sha)
    }

    fn has(&self, sha: &str) -> bool {
        let guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.contains_key(sha)
    }

    fn len(&self, sha: &str) -> Result<u64> {
        let guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(sha) {
            Some(bytes) => Ok(bytes.len() as u64),
            None => Err(Error::Plan(format!("missing blob '{sha}'"))),
        }
    }

    fn prune(&self) -> Result<usize> {
        Ok(0)
    }
}
