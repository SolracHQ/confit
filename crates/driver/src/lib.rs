//! Driver
//!
//! `std::fs` shaped file access with a memory backend under tests.
//!
//! The `fs` module holds every verb with `FsFile` handles,
//! `atomic` holds staging writes, `gzip` and `tar` and `zip`
//! hold framing over caller streams, `http` holds downloads
//! into caller streams.

#![deny(missing_docs)]

mod atomic;
/// Filesystem verbs with seekable handles.
pub mod fs;
/// Gzip single streams over caller handles.
pub mod gzip;
/// HTTP downloads over caller streams.
pub mod http;
/// Tar framing over caller streams.
pub mod tar;
/// Zip framing over caller streams.
pub mod zip;

pub use atomic::{atomic_write, copy_stream, stage_path};
