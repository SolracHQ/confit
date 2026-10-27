//! Unexpected results to required operations.

/// Unexpected filesystem result.
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
    /// Path escaping its root.
    #[error("path '{input}' escapes the project root")]
    OutsideRoot {
        /// The offending input path.
        input: String,
    },
    /// Malformed path shape.
    #[error("malformed path '{input}'")]
    MalformedPath {
        /// The offending input path.
        input: String,
    },
    /// Unmapped failure with raw text.
    #[error("{message}")]
    FileUnknown {
        /// The raw failure text.
        message: String,
    },
}

impl From<std::io::Error> for FilesystemError {
    /// Map one io failure to its filesystem cause.
    fn from(error: std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::FileMissing,
            std::io::ErrorKind::PermissionDenied => Self::FilePermissionDenied,
            std::io::ErrorKind::StorageFull => Self::DiskFull,
            std::io::ErrorKind::ReadOnlyFilesystem => Self::DiskReadOnly,
            std::io::ErrorKind::QuotaExceeded => Self::DiskQuotaExceeded,
            std::io::ErrorKind::FileTooLarge => Self::FileTooLarge,
            _ => Self::FileUnknown {
                message: error.to_string(),
            },
        }
    }
}

impl From<std::string::FromUtf8Error> for DocumentError {
    /// Map one utf8 failure to its document cause.
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::ParseFailed {
            input: error.to_string(),
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
    fn from(error: std::time::SystemTimeError) -> Self {
        Self::FileUnknown {
            message: error.to_string(),
        }
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
    #[error("unknown algorithm: {detail}")]
    AlgorithmUnknown {
        /// The algorithm detail.
        detail: String,
    },
    /// Bare gzip stream without tar.
    #[error("bare gzip stream without tar")]
    BareGzip,
    /// Corrupt archive backend.
    #[error("corrupt archive: {detail}")]
    BackendCorrupt {
        /// The corruption detail.
        detail: String,
    },
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

/// Unexpected manifest result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// Invalid manifest payload.
    #[error("invalid manifest: {detail}")]
    Invalid {
        /// The manifest detail.
        detail: String,
    },
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
    /// Folder inside the project root.
    ProjectFolder,
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
            Self::ProjectFolder => write!(formatter, "folder inside the project root"),
            Self::MemberPath => write!(formatter, "a relative member path"),
            Self::Utf8Text => write!(formatter, "utf8 text"),
        }
    }
}

/// Unexpected document result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DocumentError {
    /// Document failing its parse.
    #[error("invalid '{input}', require {require}")]
    ParseFailed {
        /// The offending input value.
        input: String,
        /// The required shape.
        require: Require,
    },
    /// Document failing its render.
    #[error("cannot render document: {reason}")]
    RenderFailed {
        /// The render reason.
        reason: String,
    },
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
    #[error("invalid project config: {detail}")]
    InvalidConfig {
        /// The config detail.
        detail: String,
    },
}

/// Unexpected eval result.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    /// Eval require cycle.
    #[error("require cycle: {}", cycle.join(" -> "))]
    CycleDetected {
        /// The ordered require chain.
        cycle: Vec<String>,
    },
    /// Repeated hook definition.
    #[error("duplicate hook '{name}'")]
    DuplicateHook {
        /// The repeated hook name.
        name: String,
    },
    /// Hook patch missing its target.
    #[error("hook patch target '{id}' matches no hook")]
    HookTargetUnknown {
        /// The unknown hook identity.
        id: String,
    },
    /// Patch target matching no document.
    #[error("patch target '{route}' matches no document")]
    PatchTargetUnknown {
        /// The unknown patch route.
        route: String,
    },
    /// Config require missing its target.
    #[error("config '{config}' requires missing '{target}': {hint}")]
    ConfigRequiresMissing {
        /// The requiring config name.
        config: String,
        /// The missing target name.
        target: String,
        /// The profile hint text.
        hint: String,
    },
    /// Changed references missing their document.
    #[error("changed references missing '{document}'")]
    ChangedReferencesMissing {
        /// The missing document name.
        document: String,
    },
    /// Require escaping its scope.
    #[error("require '{input}' escapes scope")]
    RequireEscapesScope {
        /// The offending input name.
        input: String,
    },
    /// Bad require name.
    #[error("bad require name '{input}': {rule}")]
    BadRequireName {
        /// The offending input name.
        input: String,
        /// The violated naming rule.
        rule: RequireNameRule,
    },
    /// Failed Lua evaluation.
    #[error("lua failed: {message}")]
    LuaFailed {
        /// The Lua failure text.
        message: String,
    },
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
    #[error("hook timed out after {secs}s")]
    TimedOut {
        /// The configured timeout seconds.
        secs: u64,
    },
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
    #[error("hook checks unmet: {failed:?}")]
    ChecksUnmet {
        /// The failing leaf names.
        failed: Vec<String>,
    },
    /// Hook run blocked by its requires.
    #[error("hook blocked on '{condition}': {unmet:?}")]
    Blocked {
        /// The blocking condition text.
        condition: String,
        /// The unmet leaf names.
        unmet: Vec<String>,
    },
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
    /// Store slot reason.
    #[error("invalid store slot: {0}")]
    SlotReason(SlotReason),
    /// Unreadable stdin prompt.
    #[error("Stdin unreadable: {detail}")]
    StdinUnreadable {
        /// The unreadable stdin detail.
        detail: String,
    },
}

/// Hook failure reason as a fixed fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookReason {
    /// Timed-out hook.
    TimedOut,
    /// Unreadable hook output.
    OutputUnreadable,
    /// Unresolved hook binary.
    BinaryUnresolved,
    /// Hook exit with a nonzero code.
    ExitedNonzero {
        /// The process exit code.
        code: i32,
    },
    /// Invalid hook command.
    InvalidCommand,
    /// Hook run missing its checks.
    ChecksUnmet,
    /// Hook run blocked by its requires.
    Blocked,
}

impl std::fmt::Display for HookReason {
    /// Render one hook failure reason.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TimedOut => write!(formatter, "timed out"),
            Self::OutputUnreadable => write!(formatter, "output unreadable"),
            Self::BinaryUnresolved => write!(formatter, "binary unresolved"),
            Self::ExitedNonzero { code } => write!(formatter, "exited with status {code}"),
            Self::InvalidCommand => write!(formatter, "invalid command"),
            Self::ChecksUnmet => write!(formatter, "unmet conditions"),
            Self::Blocked => write!(formatter, "blocked"),
        }
    }
}

impl HookReason {
    /// Map one hook run failure to its fixed reason.
    pub fn of(cause: &HookRunError) -> Self {
        match cause {
            HookRunError::TimedOut { .. } => Self::TimedOut,
            HookRunError::OutputUnreadable => Self::OutputUnreadable,
            HookRunError::BinaryUnresolved => Self::BinaryUnresolved,
            HookRunError::ExitedNonzero { code } => Self::ExitedNonzero { code: *code },
            HookRunError::ChecksUnmet { .. } => Self::ChecksUnmet,
            HookRunError::Blocked { .. } => Self::Blocked,
        }
    }
}

/// Evaluation detail as a closed fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalDetail {
    /// Eval failure detail.
    Eval(EvalError),
    /// Shape mismatch detail.
    Parse {
        /// The offending input value.
        input: String,
        /// The required shape.
        require: Require,
    },
    /// Face fault detail.
    Face(confit_binding::fault::FaceFault),
}

impl std::fmt::Display for EvalDetail {
    /// Render one evaluation detail.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eval(cause) => write!(formatter, "{cause}"),
            Self::Parse { input, require } => {
                write!(formatter, "invalid '{input}', require {require}")
            }
            Self::Face(fault) => {
                let path = render_face_path(&fault.segments);
                let reason = render_face_reason(&fault.reason);
                if path.is_empty() {
                    write!(formatter, "{reason}")
                } else {
                    write!(formatter, "{path}: {reason}")
                }
            }
        }
    }
}

/// Render one face fault path from the profile root.
fn render_face_path(segments: &[confit_binding::fault::Segment]) -> String {
    let mut path = String::new();
    for segment in segments {
        match segment {
            confit_binding::fault::Segment::Field(name) => {
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(name);
            }
            confit_binding::fault::Segment::Index(position) => {
                path.push_str(&format!("[{position}]"));
            }
        }
    }
    path
}

/// Render one face fault reason.
fn render_face_reason(reason: &confit_binding::fault::FaultReason) -> String {
    match reason {
        confit_binding::fault::FaultReason::UnknownVariant { found, options } => format!(
            "{found} is not a valid variant, choose one of {}",
            options.join(", ")
        ),
        confit_binding::fault::FaultReason::ShapeMismatch { require, found } => {
            format!("require {require}, found {found}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn face_detail_renders_mixed_bracket_path() {
        let detail = EvalDetail::Face(confit_binding::fault::FaceFault {
            segments: vec![
                confit_binding::fault::Segment::Field("documents".to_owned()),
                confit_binding::fault::Segment::Index(1),
                confit_binding::fault::Segment::Field("path".to_owned()),
                confit_binding::fault::Segment::Field("base".to_owned()),
            ],
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "a string".to_owned(),
                found: "integer".to_owned(),
            },
        });
        assert_eq!(
            detail.to_string(),
            "documents[1].path.base: require a string, found integer"
        );
    }

    #[test]
    fn face_detail_lists_variant_options() {
        let detail = EvalDetail::Face(confit_binding::fault::FaceFault {
            segments: vec![
                confit_binding::fault::Segment::Field("documents".to_owned()),
                confit_binding::fault::Segment::Index(1),
                confit_binding::fault::Segment::Field("path".to_owned()),
                confit_binding::fault::Segment::Field("base".to_owned()),
            ],
            reason: confit_binding::fault::FaultReason::UnknownVariant {
                found: "\"void\"".to_owned(),
                options: vec![
                    "home".to_owned(),
                    "config".to_owned(),
                    "data".to_owned(),
                    "cache".to_owned(),
                    "literal".to_owned(),
                ],
            },
        });
        assert_eq!(
            detail.to_string(),
            "documents[1].path.base: \"void\" is not a valid variant, choose one of home, config, data, cache, literal"
        );
    }

    #[test]
    fn face_detail_names_shape_mismatch() {
        let detail = EvalDetail::Face(confit_binding::fault::FaceFault {
            segments: Vec::new(),
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "a string".to_owned(),
                found: "integer".to_owned(),
            },
        });
        assert_eq!(detail.to_string(), "require a string, found integer");
    }

    #[test]
    fn binding_display_matches_terminal_renderer() {
        let fault = confit_binding::fault::FaceFault {
            segments: vec![
                confit_binding::fault::Segment::Field("documents".to_owned()),
                confit_binding::fault::Segment::Index(1),
                confit_binding::fault::Segment::Field("path".to_owned()),
                confit_binding::fault::Segment::Field("base".to_owned()),
            ],
            reason: confit_binding::fault::FaultReason::UnknownVariant {
                found: "\"void\"".to_owned(),
                options: vec!["home".to_owned(), "literal".to_owned()],
            },
        };
        assert_eq!(
            fault.to_string(),
            EvalDetail::Face(fault.clone()).to_string()
        );
    }

    #[test]
    fn face_detail_counts_indices_from_one() {
        let first = EvalDetail::Face(confit_binding::fault::FaceFault {
            segments: vec![
                confit_binding::fault::Segment::Field("documents".to_owned()),
                confit_binding::fault::Segment::Index(1),
            ],
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "a table".to_owned(),
                found: "integer".to_owned(),
            },
        });
        let second = EvalDetail::Face(confit_binding::fault::FaceFault {
            segments: vec![
                confit_binding::fault::Segment::Field("documents".to_owned()),
                confit_binding::fault::Segment::Index(2),
            ],
            reason: confit_binding::fault::FaultReason::ShapeMismatch {
                require: "a table".to_owned(),
                found: "integer".to_owned(),
            },
        });
        assert_eq!(
            first.to_string(),
            "documents[1]: require a table, found integer"
        );
        assert_eq!(
            second.to_string(),
            "documents[2]: require a table, found integer"
        );
    }
}
