//! Core
//! Shared vocabulary across crates.

#![deny(missing_docs)]

/// Shared error with cause plus context layers.
pub mod error;
/// Progress facts for one run.
pub mod progress;
/// Verified content hashes.
pub mod sha;
