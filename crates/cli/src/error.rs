//! Error
//! Failure vocabulary for command actions.

use std::path::{Path, PathBuf};

use thiserror::Error;

use confit_driver::process::ProcessFault;
use confit_engine::error::EngineError;
use confit_model::condition::Condition;
use confit_model::error::Error as ModelError;
use confit_runtime::error::RuntimeError;
use confit_store::blob::error::BlobError;
use confit_store::bundle::error::BundleError;
use confit_store::faults::AccessFault;
use confit_store::slot::error::SlotError;

use crate::presentation::hooks::describe_condition;

/// Cli failure shapes.
#[derive(Debug, Error)]
pub enum CliError {
    /// Model failure.
    #[error(transparent)]
    Model(#[from] ModelError),
    /// Engine failure.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Runtime failure.
    #[error(transparent)]
    Runtime(RuntimeError),
    /// Slot failure.
    #[error(transparent)]
    Slot(#[from] SlotError),
    /// Bundle failure.
    #[error(transparent)]
    Bundle(#[from] BundleError),
    /// Blob failure.
    #[error(transparent)]
    Blob(#[from] BlobError),
    /// Unknown failure.
    #[error("cannot use {context}: {message}")]
    Unknown {
        /// The failure context.
        context: String,
        /// The failure message for the log alone.
        message: String,
    },
    /// Failed read.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// The path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Failed write.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure.
    #[error(
        "cannot write '{path}': unexpected error, details in the log",
        path = path.display()
    )]
    WriteUnknown {
        /// The path under writing.
        path: PathBuf,
        /// The failure message for the log alone.
        message: String,
    },
    /// Empty hook argv.
    #[error("hook argv is empty")]
    EmptyArgv,
    /// Failed hook spawn.
    #[error("hook '{argv}' cannot spawn: {fault}")]
    Spawn {
        /// The joined argv under spawning.
        argv: String,
        /// The spawn fault cause.
        fault: ProcessFault,
    },
    /// Failed hook wait.
    #[error("hook '{argv}' cannot wait: {fault}")]
    Wait {
        /// The joined argv under waiting.
        argv: String,
        /// The wait fault cause.
        fault: ProcessFault,
    },
    /// Timed-out hook.
    #[error("hook '{argv}' timed out after {timeout_secs}s")]
    Timeout {
        /// The joined argv under running.
        argv: String,
        /// The run cap in seconds.
        timeout_secs: u64,
    },
    /// Failed hook output.
    #[error("hook '{argv}' cannot read output")]
    Output {
        /// The joined argv under reading.
        argv: String,
    },
    /// Unresolvable hook.
    #[error("hook '{argv}' cannot resolve '{head}': install it or extend hook path")]
    Resolve {
        /// The joined argv under resolving.
        argv: String,
        /// The binary head under resolving.
        head: String,
    },
    /// Failed hook.
    #[error("hook '{argv}' failed with code {code}")]
    HookFailed {
        /// The joined argv under running.
        argv: String,
        /// The process exit code.
        code: i32,
    },
    /// Failed hook checks.
    #[error("hook '{argv}' failed checks after run: {failed}")]
    HookChecks {
        /// The joined argv under running.
        argv: String,
        /// The unmet check names.
        failed: String,
    },
    /// Blocked hook gate.
    #[error("hook '{argv}' cannot run ({condition})")]
    HookRequires {
        /// The joined argv under blocking.
        argv: String,
        /// The closed gate condition.
        condition: String,
    },
    /// Aborted apply.
    #[error("apply aborted: answer says no 'yes'")]
    Aborted,
    /// Missing profile.
    #[error("apply says no profile '{path}'", path = path.display())]
    NoProfile {
        /// The profile path under reading.
        path: PathBuf,
    },
    /// Missing default profile.
    #[error(
        "no profile under '{project}': pass '-p PATH' or add 'main.lua'",
        project = project.display()
    )]
    NoDefaultProfile {
        /// The project folder under the run.
        project: PathBuf,
    },
    /// Profile flag with a file source.
    #[error("'-p' needs a project folder, '{path}' is a file", path = path.display())]
    ProfileWithoutProject {
        /// The file source under applying.
        path: PathBuf,
    },
    /// Present scaffold.
    #[error(
        "init: '{path}' already exists, remove it or pick another target",
        path = path.display()
    )]
    Exists {
        /// The present path under scaffolding.
        path: PathBuf,
    },
    /// Bad delete name.
    #[error(
        "delete: '{input}' is unsupported, want '@name'; history and the current slot never delete"
    )]
    BadName {
        /// The delete input under parsing.
        input: String,
    },
    /// Refused export flags.
    #[error("export: '-o' plus '--manifest' refuse together, pick one")]
    FlagRefuse,
}

impl From<RuntimeError> for CliError {
    /// Map one runtime failure onto cli language.
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::Slot(source) => Self::Slot(source),
            RuntimeError::EmptyArgv => Self::EmptyArgv,
            RuntimeError::Spawn { argv, fault } => Self::Spawn { argv, fault },
            RuntimeError::Wait { argv, fault } => Self::Wait { argv, fault },
            RuntimeError::Timeout { argv, timeout_secs } => Self::Timeout { argv, timeout_secs },
            RuntimeError::Output { argv } => Self::Output { argv },
            RuntimeError::HookResolve { argv, head } => Self::Resolve { argv, head },
            RuntimeError::HookFailed { argv, code } => Self::HookFailed { argv, code },
            RuntimeError::HookChecks { argv, failed } => Self::HookChecks {
                argv,
                failed: join_conditions(&failed),
            },
            other => Self::Runtime(other),
        }
    }
}

impl CliError {
    /// Map one read io failure at the path into domain language.
    pub fn from_read_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Read {
                path: path.to_path_buf(),
                fault,
            },
            None => Self::Unknown {
                context: format!("cannot read '{}'", path.display()),
                message,
            },
        }
    }

    /// Map one write io failure at the path into domain language.
    pub fn from_write_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Write {
                path: path.to_path_buf(),
                fault,
            },
            None => Self::WriteUnknown {
                path: path.to_path_buf(),
                message,
            },
        }
    }

    /// Render one user-visible line with detail in the log.
    pub fn terminal(&self) -> String {
        match self {
            Self::Model(_)
            | Self::Engine(_)
            | Self::Runtime(_)
            | Self::Slot(_)
            | Self::Bundle(_)
            | Self::Blob(_) => self.transport_line(),
            Self::Read { .. }
            | Self::Write { .. }
            | Self::WriteUnknown { .. }
            | Self::Unknown { .. } => self.file_line(),
            Self::Spawn { .. }
            | Self::Wait { .. }
            | Self::Timeout { .. }
            | Self::Output { .. }
            | Self::Resolve { .. }
            | Self::HookFailed { .. }
            | Self::HookChecks { .. }
            | Self::HookRequires { .. }
            | Self::EmptyArgv => self.hook_line(),
            _ => self.to_string(),
        }
    }

    /// Render one transported lower error as its task line.
    fn transport_line(&self) -> String {
        match self {
            Self::Model(error) => transport_model(error),
            Self::Engine(error) => transport_engine(error),
            Self::Runtime(error) => transport_runtime(error),
            Self::Slot(error) => transport_slot(error),
            Self::Bundle(error) => transport_bundle(error),
            Self::Blob(error) => transport_blob(error),
            _ => self.to_string(),
        }
    }

    /// Render one file failure as its task line.
    fn file_line(&self) -> String {
        match self {
            Self::Read { path, fault } => file_fault("cannot read", path, fault),
            Self::Write { path, fault } => file_fault("cannot write", path, fault),
            Self::WriteUnknown { path, message } => {
                log::error!("write '{}' failed: {message}", path.display());
                self.to_string()
            }
            Self::Unknown { context, message } => {
                log::error!("{context} failed: {message}");
                self.to_string()
            }
            _ => self.to_string(),
        }
    }

    /// Render one hook failure as its task line.
    fn hook_line(&self) -> String {
        match self {
            Self::Spawn { argv, fault } => hook_fault("spawn", argv, fault),
            Self::Wait { argv, fault } => hook_fault("wait", argv, fault),
            _ => self.to_string(),
        }
    }
}

/// Render one file fault with unknown failures in the log.
fn file_fault(op: &str, path: &Path, fault: &AccessFault) -> String {
    match fault {
        AccessFault::Unknown { message } => {
            log::error!("{op} '{}' failed: {message}", path.display());
            format!(
                "{op} '{}': unexpected error, details in the log",
                path.display()
            )
        }
        known => format!("{op} '{}': {known}", path.display()),
    }
}

/// Render one hook fault with unknown failures in the log.
fn hook_fault(op: &str, argv: &str, fault: &ProcessFault) -> String {
    match fault {
        ProcessFault::Unknown { message } => {
            log::error!("hook '{argv}' {op} failed: {message}");
            format!("hook '{argv}' cannot {op}: unexpected error, details in the log")
        }
        known => format!("hook '{argv}' cannot {op}: {known}"),
    }
}

/// Map one model error into its task line.
fn transport_model(error: &ModelError) -> String {
    match error {
        ModelError::Parse { input, want } => {
            format!("invalid '{input}': want {want}")
        }
        ModelError::Segment { input, fault } => {
            format!("invalid '{input}': want valid relative segments ({fault})")
        }
        ModelError::Render { format, reason } => {
            log::error!("model render as {format} failed: {reason}");
            format!("cannot render {format}, see log")
        }
        ModelError::Unhashable { document, reason } => {
            log::error!("cannot hash '{document}': {reason}");
            format!("cannot hash '{document}', see log")
        }
    }
}

/// Map one engine error into its task line.
fn transport_engine(error: &EngineError) -> String {
    log::error!("evaluation failed: {error}");
    "evaluation failed: unexpected error, details in the log".to_owned()
}

/// Map one runtime error into its task line.
fn transport_runtime(error: &RuntimeError) -> String {
    match error {
        RuntimeError::Slot(source) => transport_slot(source),
        RuntimeError::EmptyArgv => CliError::EmptyArgv.to_string(),
        RuntimeError::Spawn { argv, fault } => hook_fault("spawn", argv, fault),
        RuntimeError::Wait { argv, fault } => hook_fault("wait", argv, fault),
        RuntimeError::Timeout { argv, timeout_secs } => CliError::Timeout {
            argv: argv.clone(),
            timeout_secs: *timeout_secs,
        }
        .to_string(),
        RuntimeError::Output { argv } => CliError::Output { argv: argv.clone() }.to_string(),
        RuntimeError::HookResolve { argv, head } => CliError::Resolve {
            argv: argv.clone(),
            head: head.clone(),
        }
        .to_string(),
        RuntimeError::HookFailed { argv, code } => CliError::HookFailed {
            argv: argv.clone(),
            code: *code,
        }
        .to_string(),
        RuntimeError::HookChecks { argv, failed } => CliError::HookChecks {
            argv: argv.clone(),
            failed: join_conditions(failed),
        }
        .to_string(),
        RuntimeError::Render { route, reason, .. } => {
            log::error!("render '{}' failed: {reason}", route.display());
            error.to_string()
        }
        RuntimeError::WriteUnknown { path, message } => {
            log::error!("write '{}' failed: {message}", path.display());
            error.to_string()
        }
        RuntimeError::RemoveUnknown { path, message } => {
            log::error!("remove '{}' failed: {message}", path.display());
            error.to_string()
        }
        RuntimeError::MissingBlob { sha, member, .. } => {
            log::error!("missing blob '{}' for '{}'", sha.hex(), member.display());
            error.to_string()
        }
        RuntimeError::Refusal { .. } => error.to_string(),
        RuntimeError::Write { path, fault } => file_fault("cannot write", path, fault),
        RuntimeError::Remove { path, fault } => file_fault("cannot remove", path, fault),
    }
}

/// Join unmet checks into one display list.
fn join_conditions(failed: &[Condition]) -> String {
    failed
        .iter()
        .map(describe_condition)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Map one slot error into its task line.
fn transport_slot(error: &SlotError) -> String {
    use confit_store::slot::error::SlotError as Slot;
    match error {
        Slot::Read { path, fault } => file_fault("cannot read", path, fault),
        Slot::Write { path, fault } => file_fault("cannot write", path, fault),
        Slot::WriteUnknown { path, message } => {
            log::error!("write '{}' failed: {message}", path.display());
            error.to_string()
        }
        Slot::Manifest { path, source } => {
            log::error!("slot manifest '{}' failed: {source}", path.display());
            format!(
                "cannot write '{}': cannot render manifest, see log",
                path.display()
            )
        }
        _ => error.to_string(),
    }
}

/// Map one bundle error into its task line.
fn transport_bundle(error: &BundleError) -> String {
    use confit_store::bundle::error::BundleError as Bundle;
    match error {
        Bundle::Read { path, fault } => file_fault("bundle read", path, fault),
        Bundle::Write { path, fault } => file_fault("bundle write", path, fault),
        Bundle::WriteUnknown { path, message } => {
            log::error!("bundle write '{}' failed: {message}", path.display());
            error.to_string()
        }
        Bundle::Missing { sha } => {
            log::error!("missing bundle blob '{}'", sha.hex());
            format!("missing blob '{}', see log", sha.short())
        }
        Bundle::Version { got, .. } => {
            format!("bundle version {got} is unsupported")
        }
        Bundle::Manifest { path, source } => {
            log::error!("bundle manifest '{}' failed: {source}", path.display());
            format!(
                "bundle '{}' cannot render manifest, see log",
                path.display()
            )
        }
    }
}

/// Map one blob error into its task line.
fn transport_blob(error: &BlobError) -> String {
    match error {
        BlobError::Read { sha, fault } => match fault {
            AccessFault::Unknown { message } => {
                log::error!("read blob '{}' failed: {message}", sha.hex());
                format!("cannot read blob '{}', see log", sha.short())
            }
            known => {
                log::error!("read blob '{}' failed: {known}", sha.hex());
                format!("cannot read blob '{}': {known}", sha.short())
            }
        },
        BlobError::Write { path, fault } => file_fault("cannot write", path, fault),
        BlobError::WriteUnknown { path, message } => {
            log::error!("write '{}' failed: {message}", path.display());
            format!(
                "cannot write '{}': unexpected error, details in the log",
                path.display()
            )
        }
        BlobError::Corrupt { sha } => {
            log::error!("corrupt blob '{}'", sha.hex());
            format!("blob '{}' fails verification, see log", sha.short())
        }
        BlobError::Compress => "cannot compress blob".to_owned(),
    }
}

/// Cli result alias.
pub type Result<T> = std::result::Result<T, CliError>;
