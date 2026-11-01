//! Host side effects for files, archives, downloads, and processes.

#![deny(missing_docs)]

#[cfg(target_os = "windows")]
compile_error!("confit supports unix targets only");

/// Reads and writes archive containers.
pub mod archive;
mod atomic;
/// Reads and writes the host filesystem.
pub mod fs;
/// Downloads URL bodies over HTTP.
pub mod http;
/// Resolves the base folders.
pub mod paths;
/// Runs processes.
pub mod process;

pub use atomic::{StagedDir, StagedFile};
