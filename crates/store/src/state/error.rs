//! State failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// State failure shapes.
#[derive(Debug, Clone, Error)]
pub enum StateError {
    /// Failed read.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// The state path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Bad pick.
    #[error("bad state pick '{input}'")]
    BadPick {
        /// The picker input under resolving.
        input: String,
    },
    /// Failed write.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The state path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// The state path under writing.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
}

impl StateError {
    /// Map one io failure at the state path into domain language.
    pub fn from_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Read {
                path: path.to_path_buf(),
                fault,
            },
            None => Self::Read {
                path: path.to_path_buf(),
                fault: AccessFault::Unknown { message },
            },
        }
    }

    /// Map one write io failure at the state path into domain language.
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
}

/// State result alias.
pub type Result<T> = std::result::Result<T, StateError>;
