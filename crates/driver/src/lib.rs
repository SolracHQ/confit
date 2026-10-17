//! Driver
//! Side effects.

#![deny(missing_docs)]

mod atomic;
/// Filesystem helpers.
pub mod fs;
/// Gzip single streams.
pub mod gzip;
/// HTTP downloads.
pub mod http;
/// Base folders.
pub mod paths;
/// Process runs.
pub mod process;
/// Progress facts for one run.
pub mod progress;
/// Verified content hashes.
pub mod sha;
/// Tar framing.
pub mod tar;
/// Zip framing.
pub mod zip;

pub use atomic::{StagedDir, StagedFile, atomic_write, copy_stream};
