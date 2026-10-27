//! Terminal errors shown to the user.

use std::path::PathBuf;

use super::cause::{DocumentError, ErrorCause, EvalDetail, EvalError, HookReason, SlotReason};
use super::context_layer::ErrorContext;
use super::kind::ErrorKind;

/// Log closing pointing at the log file.
pub const LOG_CLOSING: &str = "For more information please see the log";
/// Guide closing pointing at help and manual.
pub const GUIDE_CLOSING: &str = "please check help or manual for detailed guide";

/// Closed terminal error result alias.
pub type Outcome<T> = std::result::Result<T, Error>;

/// Terminal error shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    kind: ErrorKind,
    chain: Option<Box<ErrorContext>>,
}

impl Error {
    /// Build one bundle read terminal error.
    pub fn read_bundle(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::ReadBundle { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one bundle write terminal error.
    pub fn write_bundle(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::WriteBundle { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one member unpack terminal error.
    pub fn unpack_member(
        member: impl Into<String>,
        required_by: Option<String>,
        chain: ErrorContext,
    ) -> Self {
        Self {
            kind: ErrorKind::UnpackMember {
                member: member.into(),
                required_by,
            },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one manifest render terminal error.
    pub fn render_manifest(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::RenderManifest,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one repeated document terminal error.
    pub fn duplicate_document(name: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::DuplicateDocument { name: name.into() },
            chain: None,
        }
    }

    /// Build one module open terminal error.
    pub fn open_module(name: impl Into<String>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::OpenModule { name: name.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one hook run terminal error.
    pub fn hook_failed(argv: impl Into<String>, reason: HookReason, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::RunHook {
                argv: argv.into(),
                reason,
            },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one hook process spawn terminal error.
    pub fn spawn_hook(argv: impl Into<String>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::SpawnHook { argv: argv.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one Lua failure terminal error.
    pub fn lua_failed(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::LuaFailed {
                message: message.into(),
            },
            chain: None,
        }
    }

    /// Build one profile evaluation terminal error.
    pub fn evaluate(detail: EvalDetail, route: Option<String>) -> Self {
        Self {
            kind: ErrorKind::Evaluate { detail, route },
            chain: None,
        }
    }

    /// Attach one log chain carrying data the line leaves out.
    pub fn with_chain(mut self, chain: ErrorContext) -> Self {
        self.chain = Some(Box::new(chain));
        self
    }

    /// Build one evaluation terminal from one difference chain.
    pub fn evaluate_chain(chain: ErrorContext) -> std::result::Result<Self, ErrorContext> {
        match chain.root_cause() {
            ErrorCause::DocumentError(DocumentError::ParseFailed { input, require }) => {
                let detail = EvalDetail::Parse {
                    input: input.clone(),
                    require: require.clone(),
                };
                Ok(Self::evaluate(detail, Self::chain_route(&chain)))
            }
            ErrorCause::EvalError(EvalError::LuaFailed { message }) => {
                Ok(Self::lua_failed(message.clone()))
            }
            ErrorCause::EvalError(cause) => {
                let detail = EvalDetail::Eval(cause.clone());
                Ok(Self::evaluate(detail, Self::chain_route(&chain)))
            }
            _ => Err(chain),
        }
    }

    /// Build one state plan terminal error.
    pub fn plan_state(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::PlanState,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one state export terminal error.
    pub fn export_state(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::ExportState,
            chain: Some(Box::new(chain)),
        }
    }

    /// Report the outermost route of one chain, when one names it.
    fn chain_route(chain: &ErrorContext) -> Option<String> {
        match chain {
            ErrorContext::Route { route, .. } => Some(route.clone()),
            ErrorContext::Leaf(_) | ErrorContext::AtPath { .. } => None,
        }
    }

    /// Build one corrupt manifest terminal error.
    pub fn corrupt_manifest(path: impl Into<PathBuf>, detail: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::CorruptManifest {
                path: path.into(),
                detail: detail.into(),
            },
            chain: None,
        }
    }

    /// Build one unsupported manifest version terminal error.
    pub fn manifest_version(path: impl Into<PathBuf>, got: u64) -> Self {
        Self {
            kind: ErrorKind::ManifestVersion {
                path: path.into(),
                got,
            },
            chain: None,
        }
    }

    /// Build one missing manifest version terminal error.
    pub fn manifest_no_version(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: ErrorKind::ManifestNoVersion { path: path.into() },
            chain: None,
        }
    }

    /// Build one non named delete terminal error.
    pub fn delete_non_named(input: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::DeleteNonNamed {
                input: input.into(),
            },
            chain: None,
        }
    }

    /// Build one conflicting flags terminal error.
    pub fn flags_conflict(command: impl Into<String>, flags: Vec<String>) -> Self {
        Self {
            kind: ErrorKind::FlagsConflict {
                command: command.into(),
                flags,
            },
            chain: None,
        }
    }

    /// Build one present target terminal error.
    pub fn overwrite_refused(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: ErrorKind::OverwriteRefused { path: path.into() },
            chain: None,
        }
    }

    /// Build one missing project terminal error.
    pub fn missing_project(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: ErrorKind::MissingProject { path: path.into() },
            chain: None,
        }
    }

    /// Build one missing profile terminal error.
    pub fn missing_profile(path: impl Into<PathBuf>) -> Self {
        Self {
            kind: ErrorKind::MissingProfile { path: path.into() },
            chain: None,
        }
    }

    /// Build one project config load terminal error.
    pub fn load_project(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::LoadProject { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one profile open terminal error.
    pub fn open_profile(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::OpenProfile { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one slot delete terminal error.
    pub fn delete_slot(slot: impl Into<String>, reason: SlotReason) -> Self {
        Self {
            kind: ErrorKind::DeleteSlot {
                slot: slot.into(),
                reason,
            },
            chain: None,
        }
    }

    /// Build one slot apply terminal error.
    pub fn apply_slot(slot: impl Into<String>, reason: SlotReason) -> Self {
        Self {
            kind: ErrorKind::ApplySlot {
                slot: slot.into(),
                reason,
            },
            chain: None,
        }
    }

    /// Build one slot export terminal error.
    pub fn export_slot(slot: impl Into<String>, reason: SlotReason) -> Self {
        Self {
            kind: ErrorKind::ExportSlot {
                slot: slot.into(),
                reason,
            },
            chain: None,
        }
    }

    /// Build one slot bundle terminal error.
    pub fn bundle_slot(slot: impl Into<String>, reason: SlotReason) -> Self {
        Self {
            kind: ErrorKind::BundleSlot {
                slot: slot.into(),
                reason,
            },
            chain: None,
        }
    }

    /// Build one state delete terminal error.
    pub fn delete_state(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::DeleteState,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one state apply terminal error.
    pub fn apply_state(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::ApplyState,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one history rotation terminal error.
    pub fn rotate_history(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::RotateHistory,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one old blob cleanup terminal error.
    pub fn prune_blobs(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::PruneBlobs,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one state store terminal error.
    pub fn store_state(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::StoreState,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one project init terminal error.
    pub fn init_project(chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::InitProject,
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one unreadable reply terminal error.
    pub fn read_reply(detail: impl Into<String>) -> Self {
        let detail = detail.into();
        Self {
            kind: ErrorKind::ReadReply {
                detail: detail.clone(),
            },
            chain: Some(Box::new(ErrorContext::from(ErrorCause::StdinUnreadable {
                detail,
            }))),
        }
    }

    /// Build one hook log write terminal error.
    pub fn write_hook_log(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::WriteHookLog { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one destination write terminal error.
    pub fn write_destination(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::WriteDestination { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Build one destination removal terminal error.
    pub fn remove_destination(path: impl Into<PathBuf>, chain: ErrorContext) -> Self {
        Self {
            kind: ErrorKind::RemoveDestination { path: path.into() },
            chain: Some(Box::new(chain)),
        }
    }

    /// Render one terminal line.
    pub fn summary(&self) -> String {
        match &self.kind {
            ErrorKind::ReadBundle { path } => {
                format!("Cannot read the bundle at '{}'", path.display())
            }
            ErrorKind::WriteBundle { path } => {
                format!("Cannot write the bundle at '{}'", path.display())
            }
            ErrorKind::UnpackMember {
                member,
                required_by,
            } => match required_by {
                Some(document) => {
                    format!("Cannot unpack the member required by document '{document}'")
                }
                None => format!("Cannot unpack the member '{member}'"),
            },
            ErrorKind::RenderManifest => "Cannot render the manifest".to_owned(),
            ErrorKind::DuplicateDocument { name } => {
                format!("Cannot render the manifest, duplicate document '{name}'")
            }
            ErrorKind::OpenModule { name } => format!("Cannot open the module '{name}'"),
            ErrorKind::RunHook { argv, reason } => match reason {
                HookReason::InvalidCommand => {
                    format!("Cannot run the hook '{argv}', invalid command")
                }
                _ => format!("Cannot run the hook '{argv}': {reason}"),
            },
            ErrorKind::SpawnHook { argv } => {
                format!("Cannot spawn hook process '{argv}'")
            }
            ErrorKind::LuaFailed { message } => {
                format!("Cannot Evaluate the profile due the following error:\n{message}")
            }
            ErrorKind::Evaluate { detail, route } => match route {
                Some(route) => format!("Cannot evaluate the profile, document '{route}', {detail}"),
                None => format!("Cannot evaluate the profile, {detail}"),
            },
            ErrorKind::CorruptManifest { path, detail } => format!(
                "Cannot load '{}' manifest, it seems like is corrupt: {detail}",
                path.display()
            ),
            ErrorKind::ManifestVersion { path, got } => format!(
                "Cannot load '{}' manifest, invalid version: {got}",
                path.display()
            ),
            ErrorKind::ManifestNoVersion { path } => {
                format!("Cannot load '{}' manifest, missing version", path.display())
            }
            ErrorKind::DeleteNonNamed { input } => {
                format!("Cannot delete non named slot '{input}'")
            }
            ErrorKind::FlagsConflict { command, flags } => format!(
                "Cannot call '{command}' with conflicting flags '{}'",
                flags.join(", ")
            ),
            ErrorKind::OverwriteRefused { path } => {
                format!("Cannot overwrite existing '{}'", path.display())
            }
            ErrorKind::MissingProject { path } => format!(
                "Cannot load project, provided dir does not exist at '{}'",
                path.display()
            ),
            ErrorKind::MissingProfile { path } => {
                format!("profile missing at '{}'", path.display())
            }
            ErrorKind::LoadProject { path } => {
                format!("Cannot load project config '{}'", path.display())
            }
            ErrorKind::OpenProfile { path } => {
                format!("Cannot open profile at '{}'", path.display())
            }
            ErrorKind::DeleteSlot { slot, reason } => {
                format!("Cannot delete the slot '{slot}', {reason}")
            }
            ErrorKind::ApplySlot { slot, reason } => {
                format!("Cannot apply the slot '{slot}', {reason}")
            }
            ErrorKind::ExportSlot { slot, reason } => {
                format!("Cannot export the slot '{slot}', {reason}")
            }
            ErrorKind::BundleSlot { slot, reason } => {
                format!("Cannot bundle the slot '{slot}', {reason}")
            }
            ErrorKind::DeleteState => "Cannot delete the state".to_owned(),
            ErrorKind::ApplyState => "Cannot apply the state".to_owned(),
            ErrorKind::PlanState => "Cannot plan the state".to_owned(),
            ErrorKind::ExportState => "Cannot export the state".to_owned(),
            ErrorKind::RotateHistory => "Cannot rotate history".to_owned(),
            ErrorKind::PruneBlobs => {
                "Cannot cleanup old blobs, unnecessary files can persist please check".to_owned()
            }
            ErrorKind::StoreState => "Cannot store the state".to_owned(),
            ErrorKind::InitProject => "Cannot initialize the project".to_owned(),
            ErrorKind::ReadReply { .. } => "Cannot read your reply".to_owned(),
            ErrorKind::WriteHookLog { path } => {
                format!("Cannot write the hook log at '{}'", path.display())
            }
            ErrorKind::WriteDestination { path } => {
                format!("Cannot write the destination at '{}'", path.display())
            }
            ErrorKind::RemoveDestination { path } => {
                format!("Cannot remove the destination at '{}'", path.display())
            }
        }
    }

    /// Report one closing line for the terminal output.
    pub fn closing(&self) -> &'static str {
        match &self.kind {
            ErrorKind::FlagsConflict { .. } => GUIDE_CLOSING,
            ErrorKind::PruneBlobs => "",
            _ => LOG_CLOSING,
        }
    }

    /// Report one log chain for remediation.
    pub fn chain(&self) -> Option<&ErrorContext> {
        self.chain.as_deref()
    }

    /// Report the innermost cause of one log chain.
    pub fn root_cause(&self) -> Option<&ErrorCause> {
        self.chain.as_deref().map(ErrorContext::root_cause)
    }
}

impl std::fmt::Display for Error {
    /// Render one terminal line for direct printing.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.summary())
    }
}

impl std::error::Error for Error {
    /// Report one log chain error.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.chain.as_deref().map(|chain| chain as _)
    }
}
