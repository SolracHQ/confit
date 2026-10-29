//! Unexpected results to required operations.
//!
//! Causes name the failed result. Location lives in [`ErrorContext`](super::ErrorContext).

/// Unexpected filesystem result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilesystemError {
    /// Vanished file.
    #[error("missing file")]
    FileMissing,
    /// Present file.
    #[error("file already exists")]
    FileExists,
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
    /// Path escaping its root.
    #[error("path escapes the project root")]
    OutsideRoot,
    /// Malformed path shape.
    #[error("malformed path")]
    MalformedPath,
    /// Unmapped failure.
    #[error("unknown filesystem failure")]
    FileUnknown,
}

impl From<std::io::Error> for FilesystemError {
    /// Map one io failure to its filesystem cause.
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::FileMissing,
            std::io::ErrorKind::AlreadyExists => Self::FileExists,
            std::io::ErrorKind::PermissionDenied => Self::FilePermissionDenied,
            std::io::ErrorKind::StorageFull => Self::DiskFull,
            std::io::ErrorKind::ReadOnlyFilesystem => Self::DiskReadOnly,
            std::io::ErrorKind::QuotaExceeded => Self::DiskQuotaExceeded,
            std::io::ErrorKind::FileTooLarge => Self::FileTooLarge,
            _ => Self::FileUnknown,
        }
    }
}

impl From<std::string::FromUtf8Error> for DocumentError {
    /// Map one utf8 failure to its document cause.
    fn from(_error: std::string::FromUtf8Error) -> Self {
        Self::ParseFailed {
            require: Require::Utf8Text,
        }
    }
}

impl From<std::string::FromUtf8Error> for ErrorCause {
    /// Map one utf8 failure to its bare shared cause.
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::from(DocumentError::from(error))
    }
}

impl From<std::time::SystemTimeError> for FilesystemError {
    /// Map one clock failure to its filesystem cause.
    fn from(_error: std::time::SystemTimeError) -> Self {
        Self::FileUnknown
    }
}

impl From<std::time::SystemTimeError> for ErrorCause {
    /// Map one clock failure to its bare shared cause.
    fn from(error: std::time::SystemTimeError) -> Self {
        Self::from(FilesystemError::from(error))
    }
}

impl From<std::io::Error> for ErrorCause {
    /// Map one io failure to its bare shared cause.
    fn from(error: std::io::Error) -> Self {
        Self::from(FilesystemError::from(error))
    }
}

/// Unexpected fetch result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// Failed download.
    #[error("download failed")]
    DownloadFailed,
    /// Rejected status code.
    #[error("status {code} rejected")]
    StatusRejected {
        /// The rejected status code.
        code: u16,
    },
    /// Timed-out request.
    #[error("request timed out")]
    TimedOut,
}

/// Unexpected archive result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchiveError {
    /// Foreign input posing as an archive.
    #[error("not a compressed archive")]
    NotArchive,
    /// Locked archive needing a password.
    #[error("password protected")]
    PasswordLocked,
    /// Unknown compression algorithm.
    #[error("unknown algorithm")]
    AlgorithmUnknown,
    /// Bare gzip stream without tar.
    #[error("bare gzip stream without tar")]
    BareGzip,
    /// Corrupt archive backend.
    #[error("corrupt archive")]
    BackendCorrupt,
}

/// Unexpected bundle result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    /// Doubled manifest entry.
    #[error("duplicate manifest entry")]
    DuplicateManifest,
    /// Missing manifest entry.
    #[error("missing manifest entry")]
    ManifestMissing,
    /// Missing content entry.
    #[error("missing content entry")]
    ContentMissing,
}

/// Store slot reason as a fixed fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotReason {
    /// Empty slot name.
    EmptyName,
    /// Slot name holding forbidden characters.
    NameChars,
    /// History slot holding a non-number.
    NotNumber,
    /// History slot beyond the stored listing.
    OutOfRange,
    /// History slot missing its entry file.
    MissingEntry,
    /// Bare slot naming neither name nor history.
    BareSlot,
}

impl std::fmt::Display for SlotReason {
    /// Render one slot reason.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(formatter, "name is empty"),
            Self::NameChars => write!(formatter, "name holds forbidden characters"),
            Self::NotNumber => write!(formatter, "history slot expects a number"),
            Self::OutOfRange => write!(formatter, "history slot is out of range"),
            Self::MissingEntry => write!(formatter, "history holds fewer entries"),
            Self::BareSlot => write!(formatter, "slot expects @name or %n"),
        }
    }
}

impl std::error::Error for SlotReason {}

/// Unexpected manifest result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// Invalid manifest payload.
    #[error("invalid manifest")]
    Invalid,
    /// Unsupported manifest version.
    #[error("manifest version {got} is unsupported")]
    VersionUnsupported {
        /// The found version number.
        got: u64,
    },
    /// Missing manifest version.
    #[error("missing manifest version")]
    MissingVersion,
}

/// Required document shape as a fixed fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Require {
    /// Non-empty route path.
    RoutePath,
    /// Resolvable base folder.
    BaseFolder,
    /// Relative member path.
    MemberPath,
    /// Utf8 text.
    Utf8Text,
}

impl std::fmt::Display for Require {
    /// Render one required shape.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoutePath => write!(formatter, "non-empty route path"),
            Self::BaseFolder => write!(formatter, "resolvable base folder"),
            Self::MemberPath => write!(formatter, "a relative member path"),
            Self::Utf8Text => write!(formatter, "utf8 text"),
        }
    }
}

/// Unexpected document result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    /// Document failing its parse.
    #[error("invalid document, require {require}")]
    ParseFailed {
        /// The required shape.
        require: Require,
    },
    /// Document failing its render.
    #[error("cannot render document")]
    RenderFailed,
    /// Opaque document refusing render.
    #[error("opaque document is not renderable")]
    OpaqueNotRenderable,
    /// Tree document refusing render.
    #[error("tree document is not renderable")]
    TreeNotRenderable,
}

/// Unexpected project load result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectError {
    /// Invalid project document.
    #[error("invalid project config")]
    InvalidConfig,
}

/// Unexpected eval result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// Repeated hook definition.
    #[error("duplicate hook")]
    DuplicateHook,
    /// Repeated document definition.
    #[error("duplicate document")]
    DuplicateDocument,
    /// Require cycle.
    #[error("require cycle")]
    CycleDetected,
    /// Hook patch missing its target.
    #[error("hook patch target matches no hook")]
    HookTargetUnknown,
    /// Patch target matching no document.
    #[error("patch target matches no document")]
    PatchTargetUnknown,
    /// Config require missing its target.
    #[error("config requires missing document")]
    ConfigRequiresMissing,
    /// Changed references missing their document.
    #[error("changed references missing document")]
    ChangedReferencesMissing,
    /// Require escaping its scope.
    #[error("require escapes scope")]
    RequireEscapesScope,
    /// Bad require name.
    #[error("bad require name: {rule}")]
    BadRequireName {
        /// The violated naming rule.
        rule: RequireNameRule,
    },
    /// Failed Lua evaluation.
    #[error("lua failed")]
    LuaFailed,
    /// Failed Face decode.
    #[error("bad shape")]
    BadShape,
}

/// Require name rule as a fixed fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequireNameRule {
    /// Backslash in the name.
    Backslash,
    /// Drive prefix in the name.
    DrivePrefix,
    /// Absolute path in the name.
    AbsolutePath,
}

impl std::fmt::Display for RequireNameRule {
    /// Render one violated naming rule.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backslash => write!(formatter, "backslashes are not allowed"),
            Self::DrivePrefix => write!(formatter, "drive prefixes are not allowed"),
            Self::AbsolutePath => write!(formatter, "absolute paths are not allowed"),
        }
    }
}

/// Unexpected hook run result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookRunError {
    /// Timed-out hook.
    #[error("hook timed out")]
    TimedOut,
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
    #[error("hook checks unmet")]
    ChecksUnmet,
    /// Hook run blocked by its requires.
    #[error("hook blocked")]
    Blocked,
}

/// Unexpected content result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// Project handle landing in cache.
    #[error("project handle in cache")]
    ProjectHandleInCache,
    /// Corrupt cached object.
    #[error("corrupt cached object '{expected}'")]
    CorruptObject {
        /// The expected content hash.
        expected: crate::sha::Sha,
    },
}

/// Unexpected command line result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    /// Conflicting flags.
    #[error("conflicting flags")]
    FlagsConflict,
}

/// Unexpected result to a required operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorCause {
    /// Filesystem failure.
    #[error(transparent)]
    FilesystemError(#[from] FilesystemError),
    /// Content failing its hash.
    #[error("content mismatch, expected '{expected}', found '{found}'")]
    ContentHashMismatch {
        /// The expected content hash.
        expected: crate::sha::Sha,
        /// The found content hash.
        found: crate::sha::Sha,
    },
    /// Content failure.
    #[error(transparent)]
    ContentError(#[from] ContentError),
    /// Fetch failure.
    #[error(transparent)]
    FetchError(#[from] FetchError),
    /// Archive failure.
    #[error(transparent)]
    ArchiveError(#[from] ArchiveError),
    /// Bundle failure.
    #[error(transparent)]
    BundleError(#[from] BundleError),
    /// Manifest failure.
    #[error(transparent)]
    ManifestError(#[from] ManifestError),
    /// Project load failure.
    #[error(transparent)]
    ProjectError(#[from] ProjectError),
    /// Document failure.
    #[error(transparent)]
    DocumentError(#[from] DocumentError),
    /// Eval failure.
    #[error(transparent)]
    EvalError(#[from] EvalError),
    /// Hook run failure.
    #[error(transparent)]
    HookRunError(#[from] HookRunError),
    /// Command line usage failure.
    #[error(transparent)]
    UsageError(#[from] UsageError),
    /// Store slot reason.
    #[error("invalid store slot: {0}")]
    SlotReason(#[from] SlotReason),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_not_found_maps_to_file_missing() {
        let failure = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert_eq!(FilesystemError::from(failure), FilesystemError::FileMissing);
    }

    #[test]
    fn io_other_maps_to_file_unknown() {
        let failure = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "pipe");
        assert_eq!(FilesystemError::from(failure), FilesystemError::FileUnknown);
    }

    #[test]
    fn status_rejected_keeps_code() {
        let cause = FetchError::StatusRejected { code: 404 };
        assert_eq!(cause.to_string(), "status 404 rejected");
    }

    #[test]
    fn bad_require_name_keeps_rule() {
        let cause = EvalError::BadRequireName {
            rule: RequireNameRule::Backslash,
        };
        assert_eq!(
            cause.to_string(),
            "bad require name: backslashes are not allowed"
        );
    }
}
