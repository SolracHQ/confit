//! Error
//! Basic causes plus context layers plus closed terminal errors.

/// Basic failure causes without location.
mod cause;
/// Context error layers for the log chain.
mod context_layer;
/// Cross-tier failures mixing mid and terminal errors.
mod failure;
/// Terminal error kinds with line data.
pub mod kind;
/// Layer wraps for results carrying convertible errors.
pub mod locate;
/// Terminal errors shown to the user.
mod terminal;

pub use cause::{
    ArchiveError, BundleError, ContentError, DocumentError, ErrorCause, EvalDetail, EvalError,
    FetchError, FilesystemError, HookReason, HookRunError, ManifestError, ProjectError, Require,
    RequireNameRule, SlotReason,
};
pub use context_layer::{ErrorContext, Result, Segment, fail};
pub use failure::{DiffWarning, Failure, Mixed, Terminal};
pub use terminal::{Error, GUIDE_CLOSING, LOG_CLOSING, Outcome};
