//! Error
//! Failure vocabulary for destination deeds.

use std::path::{Path, PathBuf};

use confit_driver::process::ProcessFault;
use confit_driver::sha::Sha;
use confit_model::condition::Condition;
use confit_model::document::StructuredFormat;
use confit_model::error::Error as ModelError;
use confit_model::routes::Route;
use thiserror::Error;

use crate::checks::Unmet;

use confit_store::archive::error::ArchiveError;
use confit_store::faults::AccessFault;
use confit_store::resources::error::ResourceError;

/// Refused payload shape for one render refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalShape {
    /// Opaque bytes.
    Opaque,
    /// Tree members.
    Tree,
}

impl std::fmt::Display for RefusalShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Opaque => f.write_str("opaque bytes"),
            Self::Tree => f.write_str("tree members"),
        }
    }
}

/// Runtime failure shapes.
#[derive(Debug, Error)]
pub enum RuntimeError {
    /// Failed write.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The destination path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure.
    #[error(
        "cannot write '{path}': unknown failure, see log",
        path = path.display()
    )]
    WriteUnknown {
        /// The destination path under writing.
        path: PathBuf,
        /// The failure message for the log alone.
        message: String,
    },
    /// Failed removal.
    #[error("cannot remove '{path}': {fault}", path = path.display())]
    Remove {
        /// The destination path under removing.
        path: PathBuf,
        /// The removal fault cause.
        fault: AccessFault,
    },
    /// Unknown removal failure.
    #[error(
        "cannot remove '{path}': unknown failure, see log",
        path = path.display()
    )]
    RemoveUnknown {
        /// The destination path under removing.
        path: PathBuf,
        /// The failure message for the log alone.
        message: String,
    },
    /// Failed render.
    #[error(
        "cannot render '{route}' as {format}, see log",
        route = route.display()
    )]
    Render {
        /// The destination route under rendering.
        route: Route,
        /// The format under rendering.
        format: StructuredFormat,
        /// The serializer reason for the log alone.
        reason: String,
    },
    /// Render refusal.
    #[error(
        "'{route}' holds {shape}, pick a matching destination",
        route = route.display()
    )]
    Refusal {
        /// The destination route under rendering.
        route: Route,
        /// The refused payload shape.
        shape: RefusalShape,
    },
    /// Missing blob.
    #[error(
        "cannot write '{dest}': member '{member}' needs blob '{short}'",
        dest = dest.display(),
        member = member.display(),
        short = sha.short()
    )]
    MissingBlob {
        /// The tree destination under writing.
        dest: PathBuf,
        /// The missing content hash.
        sha: Sha,
        /// The member path under writing.
        member: PathBuf,
    },
    /// Failed state read.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    StateRead {
        /// The state path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Corrupt state.
    #[error("cannot read '{path}': corrupt state", path = path.display())]
    StateCorrupt {
        /// The state path under reading.
        path: PathBuf,
    },
    /// Unsupported state version.
    #[error("state version {got} is unsupported")]
    StateVersion {
        /// The state path under reading.
        path: PathBuf,
        /// The seen version.
        got: u64,
    },
    /// Bad state pick.
    #[error("bad state pick '{input}'")]
    BadPick {
        /// The picker input under resolving.
        input: String,
    },
    /// Failed state manifest render.
    #[error("cannot render manifest: {source}")]
    StateManifest {
        /// The model render cause.
        source: ModelError,
    },
    /// Unknown state write failure.
    #[error("cannot write '{path}': {message}", path = path.display())]
    StateWriteUnknown {
        /// The state path under writing.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
    /// Failed bundle read.
    #[error("bundle '{path}': {fault}", path = path.display())]
    BundleRead {
        /// The bundle path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Unsupported bundle version.
    #[error("bundle version {got} is unsupported")]
    BundleVersion {
        /// The bundle path under reading.
        path: PathBuf,
        /// The seen version.
        got: u64,
    },
    /// Missing bundle blob.
    #[error("missing blob '{sha}'")]
    BundleMissing {
        /// The content hash under reading.
        sha: Sha,
    },
    /// Failed bundle write.
    #[error("bundle '{path}': {fault}", path = path.display())]
    BundleWrite {
        /// The bundle path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Failed bundle manifest render.
    #[error("bundle '{path}': cannot render manifest: {source}", path = path.display())]
    BundleManifest {
        /// The bundle path under writing.
        path: PathBuf,
        /// The model render cause.
        source: ModelError,
    },
    /// Unknown bundle write failure.
    #[error("bundle '{path}': {message}", path = path.display())]
    BundleWriteUnknown {
        /// The bundle path under writing.
        path: PathBuf,
        /// The failure message under writing.
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
    HookResolve {
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
    #[error("hook '{argv}' failed checks after run: {failed:?}")]
    HookChecks {
        /// The joined argv under running.
        argv: String,
        /// The check condition under running.
        check: Condition,
        /// The unmet fault under running.
        failed: Box<Unmet>,
    },
}

impl RuntimeError {
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

    /// Map one removal io failure at the path into domain language.
    pub fn from_remove_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Remove {
                path: path.to_path_buf(),
                fault,
            },
            None => Self::RemoveUnknown {
                path: path.to_path_buf(),
                message,
            },
        }
    }

    /// Map one io failure at the bundle path into domain language.
    pub fn from_bundle_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::BundleRead {
                path: path.to_path_buf(),
                fault,
            },
            None => Self::BundleRead {
                path: path.to_path_buf(),
                fault: AccessFault::Unknown { message },
            },
        }
    }

    /// Map one write io failure at the bundle path into domain language.
    pub fn from_bundle_write_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::BundleWrite {
                path: path.to_path_buf(),
                fault,
            },
            None => Self::BundleWriteUnknown {
                path: path.to_path_buf(),
                message,
            },
        }
    }

    /// Map one archive failure at the bundle path into domain language.
    pub fn from_archive(bundle: &Path, error: ArchiveError) -> Self {
        match error {
            ArchiveError::Read { fault, .. } => Self::BundleRead {
                path: bundle.to_path_buf(),
                fault,
            },
            ArchiveError::Write { fault, .. } => Self::BundleWrite {
                path: bundle.to_path_buf(),
                fault,
            },
            ArchiveError::WriteUnknown { message, .. } => Self::BundleWriteUnknown {
                path: bundle.to_path_buf(),
                message,
            },
            other => Self::BundleRead {
                path: bundle.to_path_buf(),
                fault: AccessFault::Unknown {
                    message: other.to_string(),
                },
            },
        }
    }

    /// Map one resource failure at the bundle path into domain language.
    pub fn from_resource(bundle: &Path, error: ResourceError) -> Self {
        match error {
            ResourceError::Read { fault, .. } => Self::BundleRead {
                path: bundle.to_path_buf(),
                fault,
            },
            other => Self::BundleRead {
                path: bundle.to_path_buf(),
                fault: AccessFault::Unknown {
                    message: other.to_string(),
                },
            },
        }
    }
}

/// Runtime result alias.
pub type Result<T> = std::result::Result<T, RuntimeError>;
