//! Failure causes with typed context and closed terminal errors.

/// Failure causes with no location.
mod cause;
/// Typed location and values for log records.
mod context;
/// Failure causes with the operation location.
mod contextual;
/// Non-blocking diff warnings.
mod failure;
/// Terminal error kinds for required operations.
pub mod kind;
/// Terminal errors shown to the user.
mod terminal;

pub use cause::{
    ArchiveError, BundleError, ContentError, DocumentError, ErrorCause, EvalError, FetchError,
    FilesystemError, HookRunError, ManifestError, ProjectError, Require, RequireNameRule,
    StateReason, UsageError,
};
pub use context::{ErrorContext, Source, SourceKind};
pub use contextual::{ContextualCause, ContextualResult, WithContext, contextual};
pub use failure::DiffWarning;
pub use terminal::{Error, GUIDE_CLOSING, LOG_CLOSING, Terminal, TerminalResult, error};
