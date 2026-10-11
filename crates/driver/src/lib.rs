//! Driver
//! `std::fs` shaped file access with a memory backend under tests.

#![deny(missing_docs)]

mod atomic;
/// Filesystem verbs with seekable handles.
pub mod fs;
/// Gzip single streams.
pub mod gzip;
/// HTTP downloads.
pub mod http;
/// Process spawns with scripted outcomes under tests.
pub mod process;
/// Progress facts for one run.
pub mod progress;
/// Verified content hashes.
pub mod sha;
/// Tar framing.
pub mod tar;
/// Zip framing.
pub mod zip;

pub use atomic::{atomic_write, copy_stream, pending_path};
