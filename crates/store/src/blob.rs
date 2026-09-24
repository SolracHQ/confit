//! Blob
//!
//! Content-addressed blob pool behind stored hashes.

use confit_core::error::Result;
use confit_core::handles::{BlobHandle, TrustedHandle};

pub mod file;
pub mod memory;

/// Content-addressed blob pool.
///
/// Pool files ride gzip bytes under stored hashes.
/// Handles seal content plus stored hashes at birth.
pub trait BlobStore {
    /// Opens a raw byte stream for one handle.
    ///
    /// Reads ride the stored hash; bytes verify against
    /// the content hash mid-stream.
    ///
    /// # Errors
    ///
    /// Missing blobs and verification failures fail as plan errors
    /// naming the hash.
    fn open(&self, handle: &BlobHandle) -> Result<Box<dyn std::io::Read>>;

    /// Stores raw bytes sealing both identities.
    ///
    /// # Errors
    ///
    /// Pool write failures surface as plan or io errors.
    fn put(&self, bytes: &[u8]) -> Result<BlobHandle>;

    /// Stores file bytes from a trusted source sealing both identities.
    ///
    /// # Errors
    ///
    /// Missing sources fail as plan errors naming the path.
    /// Pool write failures surface as plan or io errors.
    fn put_source(&self, source: &dyn TrustedHandle) -> Result<BlobHandle>;

    /// Reports whether one handle reads present.
    fn has(&self, handle: &BlobHandle) -> bool;

    /// Reads the raw byte count for one handle.
    ///
    /// # Errors
    ///
    /// Missing blobs fail as plan errors naming the hash.
    fn len(&self, handle: &BlobHandle) -> Result<u64>;

    /// Drops pool blobs unreferenced by slots and history.
    ///
    /// # Errors
    ///
    /// Listing and removal failures surface as plan or io errors.
    fn prune(&self) -> Result<usize>;
}
