//! Fetch
//!
//! Content-addressed fetch cache unifying downloads and blobs.

use confit_core::error::Result;
use confit_core::handles::{FetchHandle, Sha};
use confit_core::progress::ProgressSender;

pub mod file;
pub mod memory;

/// Content-addressed fetch cache for remote bytes.
///
/// Bodies rest under content hashes beside a `urls` index.
/// Tampered entries read as misses. Offline hits call no
/// fetcher. User shas check fatal after hit-or-download.
pub trait FetchCache {
    /// Fetches one URL into a fetch handle.
    ///
    /// # Errors
    ///
    /// Transport failures fail as plan errors. Sha mismatches
    /// fail as plan errors naming the URL.
    fn fetch(
        &self,
        url: &str,
        expected_sha: Option<Sha>,
        re_fetch: bool,
        progress: Option<&ProgressSender>,
    ) -> Result<FetchHandle>;

    /// Reads cached bytes behind one fetch handle.
    ///
    /// # Errors
    ///
    /// Missing and unreadable cache files fail as plan errors
    /// naming the origin.
    fn read(&self, handle: &FetchHandle) -> Result<Vec<u8>>;

    /// Opens cached bytes behind one fetch handle.
    ///
    /// # Errors
    ///
    /// Missing and unreadable cache files fail as plan errors
    /// naming the origin.
    fn open(&self, handle: &FetchHandle) -> Result<Box<dyn std::io::Read>>;
}
