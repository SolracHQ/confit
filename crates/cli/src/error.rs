//! Error
//! Failure vocabulary for command actions.

use std::path::{Path, PathBuf};

use thiserror::Error;

use confit_driver::process::ProcessFault;
use confit_engine::error::EngineError;
use confit_model::error::Error as ModelError;
use confit_runtime::error::RuntimeError;
use confit_store::faults::AccessFault;
use confit_store::pool::error::PoolError;
use confit_store::resources::error::ResourceError;

use crate::presentation::hooks::describe_blocked;

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
    /// Pool failure.
    #[error(transparent)]
    Pool(#[from] PoolError),
    /// Resource failure.
    #[error(transparent)]
    Resource(#[from] ResourceError),
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
    /// Unmet hook requires condition.
    #[error("hook '{argv}' cannot run ({condition})")]
    HookRequires {
        /// The joined argv under blocking.
        argv: String,
        /// The unmet requires condition.
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
    #[error("delete: '{input}' is unsupported, want '@name'")]
    BadName {
        /// The delete input under parsing.
        input: String,
    },
    /// Bad state pick.
    #[error("bad state pick '{input}'")]
    BadPicker {
        /// The picker input under parsing.
        input: String,
    },
    /// Conflicting export flags.
    #[error("export: '-o' and '--manifest' conflict, pick one")]
    FlagRefuse,
}

impl From<RuntimeError> for CliError {
    /// Map one runtime failure onto cli language.
    fn from(error: RuntimeError) -> Self {
        match error {
            RuntimeError::EmptyArgv => Self::EmptyArgv,
            RuntimeError::Spawn { argv, fault } => Self::Spawn { argv, fault },
            RuntimeError::Wait { argv, fault } => Self::Wait { argv, fault },
            RuntimeError::Timeout { argv, timeout_secs } => Self::Timeout { argv, timeout_secs },
            RuntimeError::Output { argv } => Self::Output { argv },
            RuntimeError::HookResolve { argv, head } => Self::Resolve { argv, head },
            RuntimeError::HookFailed { argv, code } => Self::HookFailed { argv, code },
            RuntimeError::HookChecks {
                argv,
                check,
                failed,
            } => Self::HookChecks {
                argv,
                failed: describe_blocked(&check, &failed),
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
            | Self::Pool(_)
            | Self::Resource(_) => self.transport_line(),
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
            Self::Pool(error) => transport_pool(error),
            Self::Resource(error) => transport_resource(error),
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
        ModelError::Render { format, reason } => {
            log::error!("model render as {format} failed: {reason}");
            format!("cannot render {format}, see log")
        }
        ModelError::Unhashable { document, reason } => {
            log::error!("cannot hash '{document}': {reason}");
            format!("cannot hash '{document}', see log")
        }
        ModelError::Base { base } => {
            log::error!("cannot resolve '{}' folder", base.name());
            format!("cannot resolve '{}' folder, see log", base.name())
        }
        ModelError::Unknown { detail } => {
            log::error!("unexpected failure: {detail}");
            "unexpected failure, see log".to_owned()
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
        RuntimeError::HookChecks {
            argv,
            check,
            failed,
        } => CliError::HookChecks {
            argv: argv.clone(),
            failed: describe_blocked(check, failed),
        }
        .to_string(),
        RuntimeError::Render { route, reason, .. } => {
            log::error!("render '{}' failed: {reason}", route.display());
            error.to_string()
        }
        RuntimeError::Expand { route, source } => {
            log::error!("resolve '{}' failed: {source}", route.display());
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
        RuntimeError::MissingContent { sha, member, .. } => {
            log::error!("missing content '{}' for '{}'", sha.hex(), member.display());
            error.to_string()
        }
        RuntimeError::Refusal { .. } => error.to_string(),
        RuntimeError::Write { path, fault } => file_fault("cannot write", path, fault),
        RuntimeError::Remove { path, fault } => file_fault("cannot remove", path, fault),
        RuntimeError::StateRead { path, fault } => file_fault("cannot read", path, fault),
        RuntimeError::StateWriteUnknown { path, message } => {
            log::error!("write '{}' failed: {message}", path.display());
            error.to_string()
        }
        RuntimeError::StateManifest { source } => {
            log::error!("state manifest failed: {source}");
            "cannot render manifest, see log".to_owned()
        }
        RuntimeError::StateCorrupt { .. }
        | RuntimeError::StateVersion { .. }
        | RuntimeError::BadPick { .. } => error.to_string(),
        RuntimeError::BundleRead { path, fault } => file_fault("bundle read", path, fault),
        RuntimeError::BundleWrite { path, fault } => file_fault("bundle write", path, fault),
        RuntimeError::BundleWriteUnknown { path, message } => {
            log::error!("bundle write '{}' failed: {message}", path.display());
            error.to_string()
        }
        RuntimeError::BundleCorrupt { .. } => error.to_string(),
        RuntimeError::BundleVersion { got, .. } => {
            format!("bundle version {got} is unsupported")
        }
        RuntimeError::BundleManifest { path, source } => {
            log::error!("bundle manifest '{}' failed: {source}", path.display());
            format!(
                "bundle '{}' cannot render manifest, see log",
                path.display()
            )
        }
    }
}

/// Map one pool error into its task line.
fn transport_pool(error: &PoolError) -> String {
    match error {
        PoolError::Write { path, fault } => file_fault("cannot write", path, fault),
        PoolError::WriteUnknown { path, message } => {
            log::error!("write '{}' failed: {message}", path.display());
            format!(
                "cannot write '{}': unexpected error, details in the log",
                path.display()
            )
        }
        PoolError::Mismatch {
            path,
            expected,
            found,
        } => {
            log::error!(
                "store '{}' failed: content mismatch, expected '{}', found '{}'",
                path.display(),
                expected.hex(),
                found.hex()
            );
            format!(
                "cannot store '{}': content mismatch, details in the log",
                path.display()
            )
        }
    }
}

/// Map one resource error into its task line.
fn transport_resource(error: &ResourceError) -> String {
    match error {
        ResourceError::Read { path, fault } => file_fault("cannot read", path, fault),
        ResourceError::Write { path, fault } => file_fault("cannot write", path, fault),
        ResourceError::WriteUnknown { path, message } => {
            log::error!("write '{}' failed: {message}", path.display());
            format!(
                "cannot write '{}': unexpected error, details in the log",
                path.display()
            )
        }
        ResourceError::Escape { path, reason } => {
            format!("cannot read '{}': {reason}", path.display())
        }
        ResourceError::Mismatch {
            path,
            expected,
            found,
        } => {
            log::error!(
                "store '{}' failed: content mismatch, expected '{}', found '{}'",
                path.display(),
                expected.hex(),
                found.hex()
            );
            format!(
                "cannot store '{}': content mismatch, details in the log",
                path.display()
            )
        }
    }
}

/// Cli result alias.
pub type Result<T> = std::result::Result<T, CliError>;
