//! Failure causes for required operations.

use crate::sha::Sha;

/// A filesystem failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FilesystemError {
    /// A file that is absent.
    #[error("missing file")]
    FileMissing,
    /// A file that is already present.
    #[error("file already exists")]
    FileExists,
    /// A path that denies access.
    #[error("permission denied")]
    FilePermissionDenied,
    /// A disk with no free space.
    #[error("disk full")]
    DiskFull,
    /// A disk that refuses writes.
    #[error("disk read-only")]
    DiskReadOnly,
    /// A disk beyond its quota.
    #[error("quota exceeded")]
    DiskQuotaExceeded,
    /// A file beyond the size limit.
    #[error("file too large")]
    FileTooLarge,
    /// A path that escapes the project root.
    #[error("path escapes the project root")]
    OutsideRoot,
    /// A path with a malformed shape.
    #[error("malformed path")]
    MalformedPath,
    /// A filesystem failure with no mapping.
    #[error("unknown filesystem failure")]
    FileUnknown,
}

impl From<std::io::Error> for FilesystemError {
    /// Turns one io failure into its filesystem cause.
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
    /// Turns one utf8 failure into its document cause.
    fn from(_error: std::string::FromUtf8Error) -> Self {
        Self::ParseFailed {
            require: Require::Utf8Text,
        }
    }
}

impl From<std::string::FromUtf8Error> for ErrorCause {
    /// Turns one utf8 failure into its bare shared cause.
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::from(DocumentError::from(error))
    }
}

impl From<std::time::SystemTimeError> for FilesystemError {
    /// Turns one clock failure into its filesystem cause.
    fn from(_error: std::time::SystemTimeError) -> Self {
        Self::FileUnknown
    }
}

impl From<std::time::SystemTimeError> for ErrorCause {
    /// Turns one clock failure into its bare shared cause.
    fn from(error: std::time::SystemTimeError) -> Self {
        Self::from(FilesystemError::from(error))
    }
}

impl From<std::io::Error> for ErrorCause {
    /// Turns one io failure into its bare shared cause.
    fn from(error: std::io::Error) -> Self {
        if let Some(cause) = error
            .get_ref()
            .and_then(|error| error.downcast_ref::<ErrorCause>())
        {
            return cause.clone();
        }
        Self::from(FilesystemError::from(error))
    }
}

/// A fetch failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// A download that failed.
    #[error("download failed")]
    DownloadFailed,
    /// A status code that the fetch rejects.
    #[error("status {code} rejected")]
    StatusRejected {
        /// The rejected status code.
        code: u16,
    },
    /// A request past its deadline.
    #[error("request timed out")]
    TimedOut,
}

/// An archive failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArchiveError {
    /// Input that is not a compressed archive.
    #[error("not a compressed archive")]
    NotArchive,
    /// An archive locked by a password.
    #[error("password protected")]
    PasswordLocked,
    /// A compression algorithm with no match.
    #[error("unknown algorithm")]
    AlgorithmUnknown,
    /// A gzip stream with no tar layer.
    #[error("bare gzip stream without tar")]
    BareGzip,
    /// An archive backend that reports corruption.
    #[error("corrupt archive")]
    BackendCorrupt,
}

/// A bundle failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    /// A manifest entry that appears twice.
    #[error("duplicate manifest entry")]
    DuplicateManifest,
    /// A manifest entry that is absent.
    #[error("missing manifest entry")]
    ManifestMissing,
    /// A content entry that is absent.
    #[error("missing content entry")]
    ContentMissing,
}

/// A stored state reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateReason {
    /// A state name that holds no characters.
    EmptyName,
    /// A state name with forbidden characters.
    ForbiddenNameChars,
    /// A history state with a non-number.
    NotNumber,
    /// A history state that points past the stored listing.
    OutOfRange,
    /// A history state with no entry file.
    MissingEntry,
    /// A state with neither a name nor a history.
    BareState,
}

impl std::fmt::Display for StateReason {
    /// Writes the state reason as text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(formatter, "name is empty"),
            Self::ForbiddenNameChars => write!(formatter, "name holds forbidden characters"),
            Self::NotNumber => write!(formatter, "history state expects a number"),
            Self::OutOfRange => write!(formatter, "history state is out of range"),
            Self::MissingEntry => write!(formatter, "history holds fewer entries"),
            Self::BareState => write!(formatter, "state expects @name or %n"),
        }
    }
}

impl std::error::Error for StateReason {}

/// A manifest failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// A manifest payload that fails its check.
    #[error("invalid manifest")]
    Invalid,
    /// A manifest version that this binary rejects.
    #[error("manifest version {got} is unsupported")]
    VersionUnsupported {
        /// The found version text.
        got: String,
    },
    /// A manifest with no version.
    #[error("missing manifest version")]
    MissingVersion,
}

/// A required document shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Require {
    /// A route path that holds at least one character.
    RoutePath,
    /// A base folder that resolves.
    BaseFolder,
    /// A relative member path.
    MemberPath,
    /// Utf8 text.
    Utf8Text,
}

impl std::fmt::Display for Require {
    /// Writes the required shape as text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RoutePath => write!(formatter, "non-empty route path"),
            Self::BaseFolder => write!(formatter, "resolvable base folder"),
            Self::MemberPath => write!(formatter, "a relative member path"),
            Self::Utf8Text => write!(formatter, "utf8 text"),
        }
    }
}

/// A document failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    /// A document that fails its parse.
    #[error("invalid document, expected {require}")]
    ParseFailed {
        /// The required shape.
        require: Require,
    },
    /// A document that fails its render.
    #[error("cannot render document")]
    RenderFailed,
    /// An opaque document that refuses render.
    #[error("opaque document is not renderable")]
    OpaqueNotRenderable,
    /// A tree document that refuses render.
    #[error("tree document is not renderable")]
    TreeNotRenderable,
}

/// A project load failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectError {
    /// A project document that fails its check.
    #[error("invalid project config")]
    InvalidConfig,
}

/// An eval failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// A hook definition that appears twice.
    #[error("duplicate hook")]
    DuplicateHook,
    /// A document definition that appears twice.
    #[error("duplicate document")]
    DuplicateDocument,
    /// A require chain that returns to its start.
    #[error("require cycle")]
    CycleDetected,
    /// A hook patch with no matching hook.
    #[error("hook patch target matches no hook")]
    HookTargetUnknown,
    /// A patch with no matching document.
    #[error("patch target matches no document")]
    PatchTargetUnknown,
    /// A config require with no matching document.
    #[error("config requires missing document")]
    ConfigRequiresMissing,
    /// A changed reference with no matching document.
    #[error("changed references missing document")]
    ChangedReferencesMissing,
    /// A require that escapes its scope.
    #[error("require escapes scope")]
    RequireEscapesScope,
    /// A require name that breaks a rule.
    #[error("invalid require name: {rule}")]
    BadRequireName {
        /// The violated naming rule.
        rule: RequireNameRule,
    },
    /// A Lua evaluation that failed.
    #[error("lua failed")]
    LuaFailed,
    /// A Shape decode that failed.
    #[error("invalid shape")]
    BadShape,
}

/// A require name rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequireNameRule {
    /// A name that holds a backslash.
    Backslash,
    /// A name that starts with a drive prefix.
    DrivePrefix,
    /// A name that holds an absolute path.
    AbsolutePath,
}

impl std::fmt::Display for RequireNameRule {
    /// Writes the violated naming rule as text.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Backslash => write!(formatter, "a name holds a backslash"),
            Self::DrivePrefix => write!(formatter, "a name holds a drive prefix"),
            Self::AbsolutePath => write!(formatter, "a name holds an absolute path"),
        }
    }
}

/// A hook execution failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HookRunError {
    /// A hook past its timeout.
    #[error("hook timed out")]
    TimedOut,
    /// A hook binary with no resolution.
    #[error("hook binary unresolved")]
    BinaryUnresolved,
    /// A hook process that exits with a nonzero code.
    #[error("hook failed with code {code}")]
    ExitedNonzero {
        /// The process exit code.
        code: i32,
    },
    /// A hook execution with unmet checks.
    #[error("hook checks unmet")]
    ChecksUnmet,
    /// A hook execution that a require blocks.
    #[error("hook blocked")]
    Blocked,
}

/// A content failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContentError {
    /// A project handle that lands in cache.
    #[error("project handle in cache")]
    ProjectHandleInCache,
    /// A cached object that fails its hash.
    #[error("corrupt cached object '{expected}'")]
    CorruptObject {
        /// The expected content hash.
        expected: Sha,
    },
}

/// A command line usage failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    /// Flags that conflict.
    #[error("conflicting flags")]
    FlagsConflict,
}

/// A failure cause for a required operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ErrorCause {
    /// A filesystem operation that failed.
    #[error(transparent)]
    FilesystemError(#[from] FilesystemError),
    /// Content that fails its hash.
    #[error("content mismatch, expected '{expected}', found '{found}'")]
    ContentHashMismatch {
        /// The expected content hash.
        expected: Sha,
        /// The found content hash.
        found: Sha,
    },
    /// A content operation that failed.
    #[error(transparent)]
    ContentError(#[from] ContentError),
    /// A fetch operation that failed.
    #[error(transparent)]
    FetchError(#[from] FetchError),
    /// An archive operation that failed.
    #[error(transparent)]
    ArchiveError(#[from] ArchiveError),
    /// A bundle operation that failed.
    #[error(transparent)]
    BundleError(#[from] BundleError),
    /// A manifest operation that failed.
    #[error(transparent)]
    ManifestError(#[from] ManifestError),
    /// A project load that failed.
    #[error(transparent)]
    ProjectError(#[from] ProjectError),
    /// A document operation that failed.
    #[error(transparent)]
    DocumentError(#[from] DocumentError),
    /// An eval operation that failed.
    #[error(transparent)]
    EvalError(#[from] EvalError),
    /// A hook execution that failed.
    #[error(transparent)]
    HookRunError(#[from] HookRunError),
    /// A command line that breaks a usage rule.
    #[error(transparent)]
    UsageError(#[from] UsageError),
    /// A stored state that breaks a rule.
    #[error("invalid stored state: {0}")]
    StateReason(#[from] StateReason),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_not_found_maps_to_file_absent() {
        let failure = std::io::Error::new(std::io::ErrorKind::NotFound, "gone");
        assert_eq!(FilesystemError::from(failure), FilesystemError::FileMissing);
    }

    #[test]
    fn io_mismatch_maps_to_content_hash_mismatch() {
        let expected = Sha::hash(b"want");
        let found = Sha::hash(b"got");
        let failure = std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            ErrorCause::ContentHashMismatch { expected, found },
        );
        match ErrorCause::from(failure) {
            ErrorCause::ContentHashMismatch {
                expected: got,
                found: seen,
            } => {
                assert_eq!(got, expected, "the cause keeps the expected hash");
                assert_eq!(seen, found, "the cause keeps the found hash");
            }
            other => panic!("expected a content hash mismatch: {other}"),
        }
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
    fn invalid_require_name_reports_the_rule() {
        let cause = EvalError::BadRequireName {
            rule: RequireNameRule::Backslash,
        };
        assert_eq!(
            cause.to_string(),
            "invalid require name: a name holds a backslash"
        );
    }
}
