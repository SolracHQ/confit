//! Pool failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use confit_driver::sha::Sha;

use crate::faults::AccessFault;

/// Pool failure shapes.
#[derive(Debug, Clone, Error)]
pub enum PoolError {
    /// Failed pool write.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The destination path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown pool write failure.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// The destination path under writing.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
    /// Mismatched stream.
    #[error(
        "cannot store '{path}': content mismatch, expected '{expected}', found '{found}'",
        path = path.display()
    )]
    Mismatch {
        /// The destination path under storing.
        path: PathBuf,
        /// The expected content hash.
        expected: Sha,
        /// The streamed content hash.
        found: Sha,
    },
}

impl PoolError {
    /// Map one pool io failure at the destination path into domain language.
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

/// Pool result alias.
pub type Result<T> = std::result::Result<T, PoolError>;
