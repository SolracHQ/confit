//! Terminal errors shown to the user.
//!
//! One error joins the required operation with its cause and its context.

use super::cause::{ErrorCause, EvalError, HookRunError, ManifestError};
use super::context::ErrorContext;
use super::contextual::{ContextualCause, ContextualResult};
use super::kind::ErrorKind;

/// Log closing pointing at the log file.
pub const LOG_CLOSING: &str = "For more information please see the log";
/// Guide closing pointing at help and manual.
pub const GUIDE_CLOSING: &str = "please check help or manual for detailed guide";

/// Closed terminal error result.
pub type TerminalResult<T> = std::result::Result<T, Error>;

/// Close one contextual cause into its terminal error.
pub fn error<T>(kind: ErrorKind, contextual: impl Into<ContextualCause>) -> TerminalResult<T> {
    Err(contextual.into().terminal(kind))
}

/// Close one contextual result into its terminal error.
pub trait Terminal<T> {
    /// Map one contextual cause into its terminal error.
    fn terminal(self, kind: ErrorKind) -> TerminalResult<T>;
}

impl<T> Terminal<T> for ContextualResult<T> {
    /// Map one contextual cause into its terminal error.
    fn terminal(self, kind: ErrorKind) -> TerminalResult<T> {
        self.map_err(|contextual| contextual.terminal(kind))
    }
}

/// Terminal error shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    cause: ErrorCause,
    context: ErrorContext,
}

impl Error {
    /// Build one terminal error.
    pub fn new(kind: ErrorKind, contextual: ContextualCause) -> Self {
        let ContextualCause { cause, context } = contextual;
        Self {
            kind,
            cause,
            context,
        }
    }

    /// Return the required operation.
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// Return the failed result.
    pub fn cause(&self) -> &ErrorCause {
        &self.cause
    }

    /// Return where the operation ran.
    pub fn context(&self) -> &ErrorContext {
        &self.context
    }

    /// Render one terminal line.
    pub fn summary(&self) -> String {
        let context = &self.context;
        match &self.kind {
            ErrorKind::ReadBundle => match &context.bundle_path {
                Some(path) => format!("Cannot read the bundle at '{}'", path.display()),
                None => "Cannot read the bundle at 'unknown'".to_owned(),
            },
            ErrorKind::WriteBundle => match &context.bundle_path {
                Some(path) => format!("Cannot write the bundle at '{}'", path.display()),
                None => "Cannot write the bundle at 'unknown'".to_owned(),
            },
            ErrorKind::UnpackMember => match &context.document {
                Some(document) => {
                    format!("Cannot unpack the member required by document '{document}'")
                }
                None => match &context.member {
                    Some(member) => format!("Cannot unpack the member '{member}'"),
                    None => "Cannot unpack the member 'unknown'".to_owned(),
                },
            },
            ErrorKind::RenderManifest => "Cannot render the manifest".to_owned(),
            ErrorKind::DuplicateDocument => match &context.document {
                Some(document) => {
                    format!("Cannot render the manifest, duplicate document '{document}'")
                }
                None => "Cannot render the manifest, duplicate document 'unknown'".to_owned(),
            },
            ErrorKind::OpenModule => match &context.module {
                Some(module) => format!("Cannot open the module '{module}'"),
                None => "Cannot open the module 'unknown'".to_owned(),
            },
            ErrorKind::RunHook => match &context.argv {
                Some(argv) => match &self.cause {
                    ErrorCause::HookRunError(HookRunError::BinaryUnresolved) => {
                        format!("Cannot run the hook '{argv}', invalid command")
                    }
                    _ => format!("Cannot run the hook '{argv}': {}", self.cause),
                },
                None => format!("Cannot run the hook: {}", self.cause),
            },
            ErrorKind::SpawnHook => match &context.argv {
                Some(argv) => format!("Cannot spawn hook process '{argv}'"),
                None => "Cannot spawn hook process 'unknown'".to_owned(),
            },
            ErrorKind::LuaFailed => match &context.detail {
                Some(detail) => {
                    let mut text =
                        format!("Cannot Evaluate the profile due the following error:\n{detail}");
                    if !context.trace.is_empty() {
                        text.push('\n');
                        text.push_str(&context.trace.join("\n"));
                    }
                    text
                }
                None => "Cannot Evaluate the profile due the following error:\nunknown".to_owned(),
            },
            ErrorKind::Evaluate => {
                if let (ErrorCause::EvalError(EvalError::HookTargetUnknown), Some(id)) =
                    (&self.cause, &context.id)
                {
                    return format!(
                        "Cannot evaluate the profile, hook patch target '{id}' matches no hook"
                    );
                }
                let text = match &context.detail {
                    Some(detail) => detail.clone(),
                    None => self.cause.to_string(),
                };
                match &context.document {
                    Some(document) => {
                        format!("Cannot evaluate the profile, document '{document}', {text}")
                    }
                    None => format!("Cannot evaluate the profile, {text}"),
                }
            }
            ErrorKind::CorruptManifest => match (&context.fs_path, &context.detail) {
                (Some(path), Some(detail)) => format!(
                    "Cannot load '{}' manifest, it seems like is corrupt: {detail}",
                    path.display()
                ),
                (Some(path), None) => format!(
                    "Cannot load '{}' manifest, it seems like is corrupt: unknown",
                    path.display()
                ),
                (None, _) => {
                    "Cannot load 'unknown' manifest, it seems like is corrupt: unknown".to_owned()
                }
            },
            ErrorKind::ManifestVersion => match &self.cause {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    match &context.fs_path {
                        Some(path) => format!(
                            "Cannot load '{}' manifest, invalid version: {got}",
                            path.display()
                        ),
                        None => format!("Cannot load 'unknown' manifest, invalid version: {got}"),
                    }
                }
                _ => match &context.fs_path {
                    Some(path) => {
                        format!("Cannot load '{}' manifest: {}", path.display(), self.cause)
                    }
                    None => format!("Cannot load 'unknown' manifest: {}", self.cause),
                },
            },
            ErrorKind::ManifestNoVersion => match &context.fs_path {
                Some(path) => format!("Cannot load '{}' manifest, missing version", path.display()),
                None => "Cannot load 'unknown' manifest, missing version".to_owned(),
            },
            ErrorKind::DeleteNonNamed => match &context.slot {
                Some(slot) => format!("Cannot delete non named slot '{slot}'"),
                None => "Cannot delete non named slot 'unknown'".to_owned(),
            },
            ErrorKind::FlagsConflict => match &context.command {
                Some(command) => format!(
                    "Cannot call '{command}' with conflicting flags '{}'",
                    context.flags.join(", ")
                ),
                None => format!(
                    "Cannot call 'unknown' with conflicting flags '{}'",
                    context.flags.join(", ")
                ),
            },
            ErrorKind::OverwriteRefused => match &context.fs_path {
                Some(path) => format!("Cannot overwrite existing '{}'", path.display()),
                None => "Cannot overwrite existing 'unknown'".to_owned(),
            },
            ErrorKind::MissingProject => match &context.project {
                Some(project) => format!(
                    "Cannot load project, provided dir does not exist at '{}'",
                    project.display()
                ),
                None => "Cannot load project, provided dir does not exist at 'unknown'".to_owned(),
            },
            ErrorKind::MissingProfile => match &context.profile {
                Some(profile) => format!("profile missing at '{}'", profile.display()),
                None => "profile missing at 'unknown'".to_owned(),
            },
            ErrorKind::LoadProject => match (&context.project, &context.fs_path) {
                (Some(project), _) => {
                    format!("Cannot load project config '{}'", project.display())
                }
                (None, Some(path)) => {
                    format!("Cannot load project config '{}'", path.display())
                }
                (None, None) => "Cannot load project config 'unknown'".to_owned(),
            },
            ErrorKind::OpenProfile => match &context.profile {
                Some(profile) => format!("Cannot open profile at '{}'", profile.display()),
                None => "Cannot open profile at 'unknown'".to_owned(),
            },
            ErrorKind::DeleteSlot
            | ErrorKind::RestoreSlot
            | ErrorKind::ExportSlot
            | ErrorKind::BundleSlot => {
                let verb = match &self.kind {
                    ErrorKind::DeleteSlot => "delete",
                    ErrorKind::RestoreSlot => "restore",
                    ErrorKind::ExportSlot => "export",
                    _ => "bundle",
                };
                match &context.slot {
                    Some(slot) => format!("Cannot {verb} the slot '{slot}', {}", self.cause),
                    None => format!("Cannot {verb} the slot 'unknown', {}", self.cause),
                }
            }
            ErrorKind::DeleteState => "Cannot delete the state".to_owned(),
            ErrorKind::ApplyState => "Cannot apply the state".to_owned(),
            ErrorKind::BundleState => "Cannot bundle the state".to_owned(),
            ErrorKind::ExportState => "Cannot export the state".to_owned(),
            ErrorKind::RestoreState => "Cannot restore the state".to_owned(),
            ErrorKind::RotateHistory => "Cannot rotate history".to_owned(),
            ErrorKind::PruneBlobs => {
                "Cannot cleanup old blobs, unnecessary files can persist please check".to_owned()
            }
            ErrorKind::StoreState => "Cannot store the state".to_owned(),
            ErrorKind::InitProject => "Cannot initialize the project".to_owned(),
            ErrorKind::ReadReply => "Cannot read your reply".to_owned(),
            ErrorKind::WriteHookLog => match &context.fs_path {
                Some(path) => format!("Cannot write the hook log at '{}'", path.display()),
                None => "Cannot write the hook log at 'unknown'".to_owned(),
            },
            ErrorKind::WriteDestination => match &context.fs_path {
                Some(path) => format!("Cannot write the destination at '{}'", path.display()),
                None => "Cannot write the destination at 'unknown'".to_owned(),
            },
            ErrorKind::RemoveDestination => match &context.fs_path {
                Some(path) => format!("Cannot remove the destination at '{}'", path.display()),
                None => "Cannot remove the destination at 'unknown'".to_owned(),
            },
        }
    }

    /// Report one closing line for the terminal output.
    pub fn closing(&self) -> &'static str {
        match &self.kind {
            ErrorKind::FlagsConflict => GUIDE_CLOSING,
            ErrorKind::PruneBlobs => "",
            _ => LOG_CLOSING,
        }
    }
}

impl std::fmt::Display for Error {
    /// Render one terminal line for direct printing.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.summary())
    }
}

impl std::error::Error for Error {
    /// Report one cause.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}
