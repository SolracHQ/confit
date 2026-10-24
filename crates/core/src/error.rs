//! Error
//! Shared cause plus location stack.

use std::path::PathBuf;

use crate::sha::Sha;

/// Filesystem failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilesystemError {
    /// Vanished file.
    #[error("missing file")]
    FileMissing,
    /// Refused file.
    #[error("permission denied")]
    FilePermissionDenied,
    /// Full disk.
    #[error("disk full")]
    DiskFull,
    /// Read-only disk.
    #[error("disk read-only")]
    DiskReadOnly,
    /// Capped disk.
    #[error("quota exceeded")]
    DiskQuotaExceeded,
    /// Oversized file.
    #[error("file too large")]
    FileTooLarge,
    /// Unmapped failure with raw text.
    #[error("{message}")]
    FileUnknown {
        /// The raw failure text.
        message: String,
    },
}

/// Fetch failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// Failed download.
    #[error("download failed: {reason}")]
    DownloadFailed {
        /// The failure reason.
        reason: String,
    },
    /// Rejected status code.
    #[error("status {code} rejected")]
    StatusRejected {
        /// The rejected status code.
        code: u16,
    },
    /// Timed-out request.
    #[error("request timed out")]
    TimedOut,
    /// Missing scripted body.
    #[error("no scripted body")]
    Unscripted,
}

/// Archive failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchiveError {
    /// Foreign input posing as an archive.
    #[error("not a compressed archive")]
    NotArchive,
    /// Missing member name.
    #[error("unknown member '{name}'")]
    MemberMissing {
        /// The missing member name.
        name: String,
    },
    /// Corrupt archive envelope.
    #[error("corrupt archive: {detail}")]
    EnvelopeCorrupt {
        /// The corruption detail.
        detail: String,
    },
    /// Locked archive needing a password.
    #[error("password protected")]
    PasswordLocked,
    /// Unknown compression algorithm.
    #[error("unknown algorithm: {detail}")]
    AlgorithmUnknown {
        /// The algorithm detail.
        detail: String,
    },
}

/// Bundle failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    /// Corrupt bundle envelope.
    #[error("corrupt bundle: {detail}")]
    EnvelopeCorrupt {
        /// The corruption detail.
        detail: String,
    },
    /// Invalid bundle manifest.
    #[error("invalid manifest: {detail}")]
    ManifestInvalid {
        /// The manifest detail.
        detail: String,
    },
    /// Unsupported bundle version.
    #[error("bundle version {got} is unsupported")]
    VersionUnsupported {
        /// The found version number.
        got: u64,
    },
}

/// State failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    /// Unsupported state version.
    #[error("state version {got} is unsupported")]
    VersionUnsupported {
        /// The found version number.
        got: u64,
    },
    /// Corrupt state entry.
    #[error("corrupt state entry: {detail}")]
    EntryCorrupt {
        /// The corruption detail.
        detail: String,
    },
    /// Invalid state pick.
    #[error("bad state pick '{input}'")]
    PickInvalid {
        /// The offending pick input.
        input: String,
    },
}

/// Document failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    /// Document failing its parse.
    #[error("invalid '{input}': want {want}")]
    ParseFailed {
        /// The offending input value.
        input: String,
        /// The wanted shape.
        want: String,
    },
    /// Document failing its render.
    #[error("cannot render document: {reason}")]
    RenderFailed {
        /// The render reason.
        reason: String,
    },
    /// Document refusing its hash.
    #[error("cannot hash document: {reason}")]
    Unhashable {
        /// The hash reason.
        reason: String,
    },
}

/// Eval failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// Unknown eval module.
    #[error("unknown module '{name}'")]
    ModuleMissing {
        /// The missing module name.
        name: String,
    },
    /// Eval require cycle.
    #[error("require cycle: {cycle}")]
    CycleDetected {
        /// The cycle display.
        cycle: String,
    },
    /// Repeated eval definition.
    #[error("'{name}' is already defined")]
    DuplicateDefined {
        /// The repeated name.
        name: String,
    },
    /// Hook patch missing its target.
    #[error("hook patch target '{id}' matches no hook")]
    HookTargetUnknown {
        /// The unknown hook identity.
        id: String,
    },
    /// Unsatisfied eval require.
    #[error("unsatisfied require: {detail}")]
    RequireUnsatisfied {
        /// The require detail.
        detail: String,
    },
    /// Failed Lua evaluation.
    #[error("lua failed: {message}")]
    LuaFailed {
        /// The Lua failure text.
        message: String,
    },
}

/// Hook run failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookRunError {
    /// Failed hook spawn.
    #[error("hook spawn failed")]
    SpawnFailed,
    /// Failed hook wait.
    #[error("hook wait failed")]
    WaitFailed,
    /// Timed-out hook.
    #[error("hook timed out")]
    TimedOut,
    /// Unreadable hook output.
    #[error("hook output unreadable")]
    OutputUnreadable,
    /// Unresolved hook binary.
    #[error("hook binary unresolved")]
    BinaryUnresolved,
    /// Hook exit with a nonzero code.
    #[error("hook failed with code {code}")]
    ExitedNonzero {
        /// The process exit code.
        code: i32,
    },
    /// Hook run missing its checks.
    #[error("hook checks unmet: {detail}")]
    ChecksUnmet {
        /// The unmet checks detail.
        detail: String,
    },
    /// Hook run blocked by its requires.
    #[error("hook blocked: {detail}")]
    Blocked {
        /// The block detail.
        detail: String,
    },
}

/// User action failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UserActionError {
    /// Aborted user action.
    #[error("action aborted")]
    Aborted,
    /// Missing profile.
    #[error("profile missing")]
    ProfileMissing,
    /// Missing project.
    #[error("project missing")]
    ProjectMissing,
    /// Present target.
    #[error("target exists")]
    TargetExists,
    /// Invalid name.
    #[error("invalid name")]
    NameInvalid,
    /// Conflicting flags.
    #[error("conflicting flags")]
    FlagsConflict,
}

/// Shared failure cause without location.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Cause {
    /// Filesystem failure.
    #[error(transparent)]
    FilesystemError(FilesystemError),
    /// Content failing its hash.
    #[error("content mismatch, expected '{expected}', found '{found}'")]
    ContentHashMismatch {
        /// The expected content hash.
        expected: Sha,
        /// The found content hash.
        found: Sha,
    },
    /// Fetch failure.
    #[error(transparent)]
    FetchError(FetchError),
    /// Archive failure.
    #[error(transparent)]
    ArchiveError(ArchiveError),
    /// Bundle failure.
    #[error(transparent)]
    BundleError(BundleError),
    /// State failure.
    #[error(transparent)]
    StateError(StateError),
    /// Document failure.
    #[error(transparent)]
    DocumentError(DocumentError),
    /// Eval failure.
    #[error(transparent)]
    EvalError(EvalError),
    /// Hook run failure.
    #[error(transparent)]
    HookRunError(HookRunError),
    /// User action failure.
    #[error(transparent)]
    UserActionError(UserActionError),
}

/// Location entry for one error stack layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Filesystem path under use.
    Path(PathBuf),
    /// Remote address under fetching.
    Url(String),
    /// Bundle path under packing.
    Bundle(PathBuf),
    /// Archive member name under unpacking.
    Member(String),
    /// Document route under rendering.
    Route(String),
    /// Hook argv text under running.
    Argv(String),
}

/// Shared error with one cause plus a location stack.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{cause}")]
pub struct Error {
    /// The location-free failure cause.
    pub cause: Cause,
    /// The location stack with the oldest entry first.
    pub stack: Vec<Location>,
}

/// Shared error result alias.
pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    /// Build one error from one cause.
    pub fn new(cause: Cause) -> Self {
        Self {
            cause,
            stack: Vec::new(),
        }
    }

    /// Push one path location and return the error.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.stack.push(Location::Path(path.into()));
        self
    }

    /// Push one url location and return the error.
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.stack.push(Location::Url(url.into()));
        self
    }

    /// Push one bundle location and return the error.
    pub fn with_bundle(mut self, bundle: impl Into<PathBuf>) -> Self {
        self.stack.push(Location::Bundle(bundle.into()));
        self
    }

    /// Push one member location and return the error.
    pub fn with_member(mut self, member: impl Into<String>) -> Self {
        self.stack.push(Location::Member(member.into()));
        self
    }

    /// Push one route location and return the error.
    pub fn with_route(mut self, route: impl Into<String>) -> Self {
        self.stack.push(Location::Route(route.into()));
        self
    }

    /// Push one argv location and return the error.
    pub fn with_argv(mut self, argv: impl Into<String>) -> Self {
        self.stack.push(Location::Argv(argv.into()));
        self
    }
}
