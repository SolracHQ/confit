//! Driver side effects.

#![deny(missing_docs)]

#[cfg(target_os = "windows")]
compile_error!("confit supports unix targets only");

/// Archive framing.
pub mod archive;
mod atomic;
/// Filesystem helpers.
pub mod fs;
/// HTTP downloads.
pub mod http;
/// Base folders.
pub mod paths;
/// Process runs.
pub mod process;

pub use atomic::{StagedDir, StagedFile};
