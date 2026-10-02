//! Error
//!
//! Failure vocabulary for command actions.

use std::path::{Path, PathBuf};

use thiserror::Error;

use confit_engine::error::EngineError;
use confit_model::error::Error as ModelError;
use confit_runtime::error::RuntimeError;
use confit_store::blob::error::BlobError;
use confit_store::bundle::error::BundleError;
use confit_store::faults::AccessFault;
use confit_store::slot::error::SlotError;

/// Cli failure shapes.
#[derive(Debug, Error)]
pub enum CliError {
    /// Model failure holding the nested model error.
    #[error(transparent)]
    Model(#[from] ModelError),
    /// Engine failure holding the nested engine error.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// Runtime failure holding the nested runtime error.
    #[error(transparent)]
    Runtime(#[from] RuntimeError),
    /// Slot failure holding the nested slot error.
    #[error(transparent)]
    Slot(#[from] SlotError),
    /// Bundle failure holding the nested bundle error.
    #[error(transparent)]
    Bundle(#[from] BundleError),
    /// Blob failure holding the nested blob error.
    #[error(transparent)]
    Blob(#[from] BlobError),
    /// Unknown failure holding its context with the message.
    #[error("cannot use {context}: {message}")]
    Unknown {
        /// Holds the failure context.
        context: String,
        /// Holds the failure message for the log alone.
        message: String,
    },
    /// Failed read holding the path with the fault.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// Holds the path under reading.
        path: PathBuf,
        /// Holds the read fault cause.
        fault: AccessFault,
    },
    /// Failed write holding the path with the fault.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// Holds the path under writing.
        path: PathBuf,
        /// Holds the write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure holding the path.
    #[error(
        "cannot write '{path}': unexpected error, details in the log",
        path = path.display()
    )]
    WriteUnknown {
        /// Holds the path under writing.
        path: PathBuf,
        /// Holds the failure message for the log alone.
        message: String,
    },
    /// Empty hook argv holding nothing.
    #[error("hook argv reads empty")]
    EmptyArgv,
    /// Failed hook spawn holding argv.
    #[error("hook '{argv}' cannot spawn, see log")]
    Spawn {
        /// Holds the joined argv under spawning.
        argv: String,
        /// Holds the spawn failure message for the log alone.
        message: String,
    },
    /// Failed hook wait holding argv.
    #[error("hook '{argv}' cannot wait, see log")]
    Wait {
        /// Holds the joined argv under waiting.
        argv: String,
        /// Holds the wait failure message for the log alone.
        message: String,
    },
    /// Timed-out hook holding argv with the cap.
    #[error("hook '{argv}' timed out after {timeout_secs}s")]
    Timeout {
        /// Holds the joined argv under running.
        argv: String,
        /// Holds the run cap in seconds.
        timeout_secs: u64,
    },
    /// Failed hook output holding argv alone.
    #[error("hook '{argv}' cannot read output")]
    Output {
        /// Holds the joined argv under reading.
        argv: String,
    },
    /// Unresolvable hook holding argv with the head.
    #[error("hook '{argv}' cannot resolve '{head}': install it or extend hook path")]
    Resolve {
        /// Holds the joined argv under resolving.
        argv: String,
        /// Holds the binary head under resolving.
        head: String,
    },
    /// Failed hook holding argv with the code.
    #[error("hook '{argv}' failed with code {code}")]
    HookFailed {
        /// Holds the joined argv under running.
        argv: String,
        /// Holds the process exit code.
        code: i32,
    },
    /// Failed hook checks holding argv with the names.
    #[error("hook '{argv}' failed checks after run: {failed}")]
    HookChecks {
        /// Holds the joined argv under running.
        argv: String,
        /// Holds the unmet check names.
        failed: String,
    },
    /// Aborted apply holding nothing.
    #[error("apply aborted: answer reads no 'yes'")]
    Aborted,
    /// Missing profile holding the path.
    #[error("apply reads no profile '{path}'", path = path.display())]
    NoProfile {
        /// Holds the profile path under reading.
        path: PathBuf,
    },
    /// Present scaffold holding the path.
    #[error(
        "init: '{path}' already exists, remove it or pick another target",
        path = path.display()
    )]
    Exists {
        /// Holds the present path under scaffolding.
        path: PathBuf,
    },
    /// Bad delete name holding the input.
    #[error(
        "delete: '{input}' reads unsupported, want '@name'; history and the current slot never delete"
    )]
    BadName {
        /// Holds the delete input under parsing.
        input: String,
    },
    /// Refused export flags holding nothing.
    #[error("export: '-o' plus '--manifest' refuse together, pick one")]
    FlagRefuse,
}

impl CliError {
    /// Maps one read io failure at the path into domain language.
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

    /// Maps one write io failure at the path into domain language.
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

    /// Renders one user-visible line with detail in the log.
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
            | Self::EmptyArgv => self.hook_line(),
            _ => self.to_string(),
        }
    }

    /// Renders one transported lower error as its task line.
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

    /// Renders one file failure as its task line.
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

    /// Renders one hook failure as its task line.
    fn hook_line(&self) -> String {
        match self {
            Self::Spawn { argv, message } => {
                log::error!("hook '{argv}' spawn failed: {message}");
                self.to_string()
            }
            Self::Wait { argv, message } => {
                log::error!("hook '{argv}' wait failed: {message}");
                self.to_string()
            }
            _ => self.to_string(),
        }
    }
}

/// Renders one file fault with unknown failures in the log.
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

/// Maps one model error into its task line.
fn transport_model(error: &ModelError) -> String {
    match error {
        ModelError::Parse { input, want } => {
            format!("invalid '{input}': want {want}")
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

/// Maps one engine error into its task line.
fn transport_engine(error: &EngineError) -> String {
    log::error!("evaluation failed: {error}");
    "evaluation failed: unexpected error, details in the log".to_owned()
}

/// Maps one runtime error into its task line.
fn transport_runtime(error: &RuntimeError) -> String {
    match error {
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

/// Maps one slot error into its task line.
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

/// Maps one bundle error into its task line.
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
            format!("bundle version {got} reads unsupported")
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

/// Maps one blob error into its task line.
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
