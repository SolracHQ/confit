//! Core
//! Shared vocabulary across crates.

#![deny(missing_docs)]

/// Shared error with cause plus context layers.
pub mod error;
/// Verified content hashes.
pub mod sha;
/// Near-match suggestions for unknown names.
pub mod suggest;
/// Binary version triplet with manifest compatibility.
pub mod version;
