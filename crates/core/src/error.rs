//! Error
//! Bare causes plus typed context plus closed terminal errors.

/// Basic failure causes without location.
mod cause;
/// Typed error context for log records.
mod context;
/// Contextual causes carrying context.
mod contextual;
/// Non-blocking diff warnings.
mod failure;
/// Terminal error kinds naming required operations.
pub mod kind;
/// Terminal errors shown to the user.
mod terminal;

pub use cause::{
    ArchiveError, BundleError, ContentError, DocumentError, ErrorCause, EvalError, FetchError,
    FilesystemError, HookRunError, ManifestError, ProjectError, Require, RequireNameRule,
    SlotReason, UsageError,
};
pub use context::{ErrorContext, Source, SourceKind};
pub use contextual::{ContextualCause, ContextualResult, WithContext, contextual};
pub use failure::DiffWarning;
pub use terminal::{Error, GUIDE_CLOSING, LOG_CLOSING, Terminal, TerminalResult, error};
