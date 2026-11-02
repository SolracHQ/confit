//! Shared vocabulary across crates.

#![deny(missing_docs)]

/// Shared failures with causes, context, and terminal kinds.
pub mod error;
/// Validated content hashes for trusted stores.
pub mod resource_ref;
/// Verified content hashes.
pub mod sha;
/// Near-match suggestions for unknown names.
pub mod suggest;
/// The binary version and the manifest versions it loads.
pub mod version;
