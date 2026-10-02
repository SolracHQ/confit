//! Error
//!
//! Failure vocabulary for destination deeds.

use std::path::{Path, PathBuf};

use confit_model::document::StructuredFormat;
use confit_model::routes::Route;
use confit_model::sha::Sha;
use thiserror::Error;

use confit_store::faults::AccessFault;

/// Refused payload shape behind one render refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalShape {
    /// Opaque bytes riding the blob store.
    Opaque,
    /// Tree members riding member files.
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
    /// Failed write holding the destination path with the fault.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// Holds the destination path under writing.
        path: PathBuf,
        /// Holds the write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure holding the destination.
    #[error(
        "cannot write '{path}': unknown failure, see log",
        path = path.display()
    )]
    WriteUnknown {
        /// Holds the destination path under writing.
        path: PathBuf,
        /// Holds the failure message for the log alone.
        message: String,
    },
    /// Failed removal holding the destination with the fault.
    #[error("cannot remove '{path}': {fault}", path = path.display())]
    Remove {
        /// Holds the destination path under removing.
        path: PathBuf,
        /// Holds the removal fault cause.
        fault: AccessFault,
    },
    /// Unknown removal failure holding the path.
    #[error(
        "cannot remove '{path}': unknown failure, see log",
        path = path.display()
    )]
    RemoveUnknown {
        /// Holds the destination path under removing.
        path: PathBuf,
        /// Holds the failure message for the log alone.
        message: String,
    },
    /// Failed render holding the route with format.
    #[error(
        "cannot render '{route}' as {format}, see log",
        route = route.display()
    )]
    Render {
        /// Holds the destination route under rendering.
        route: Route,
        /// Holds the format under rendering.
        format: StructuredFormat,
        /// Holds the serializer reason for the log alone.
        reason: String,
    },
    /// Render refusal holding the route with the shape.
    #[error(
        "'{route}' holds {shape}, pick a matching destination",
        route = route.display()
    )]
    Refusal {
        /// Holds the destination route under rendering.
        route: Route,
        /// Holds the refused payload shape.
        shape: RefusalShape,
    },
    /// Missing blob holding the destination with the hash and member.
    #[error(
        "cannot write '{dest}': member '{member}' needs blob '{short}'",
        dest = dest.display(),
        member = member.display(),
        short = sha.short()
    )]
    MissingBlob {
        /// Holds the tree destination under writing.
        dest: PathBuf,
        /// Holds the missing content hash.
        sha: Sha,
        /// Holds the member path under writing.
        member: PathBuf,
    },
}

impl RuntimeError {
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

    /// Maps one removal io failure at the path into domain language.
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
}

/// Runtime result alias.
pub type Result<T> = std::result::Result<T, RuntimeError>;
