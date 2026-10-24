//! Core
//! Shared vocabulary across crates.

#![deny(missing_docs)]

/// Shared error with cause plus location stack.
pub mod error;
/// Progress facts for one run.
pub mod progress;
/// Verified content hashes.
pub mod sha;

pub use error::{
    ArchiveError, BundleError, Cause, DocumentError, Error, EvalError, FetchError, FilesystemError,
    HookRunError, Location, StateError, UserActionError,
};
pub use progress::{Event, ProgressSender};
pub use sha::{SHA_BYTE_LEN, Sha, ShaError, ShaWriter};
