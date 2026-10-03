//! Blob failures.

use std::path::{Path, PathBuf};

use confit_model::sha::Sha;
use thiserror::Error;

use crate::faults::AccessFault;

/// Blob failure shapes.
#[derive(Debug, Clone, Error)]
pub enum BlobError {
    /// Failed read holding the content hash with the fault.
    #[error("cannot read blob '{sha}': {fault}")]
    Read {
        /// The content hash under reading.
        sha: Sha,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Failed write holding the destination path with the fault.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The destination path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure holding the path with message.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// The destination path under writing.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
    /// Corrupt pool entry holding the content hash.
    #[error("blob '{sha}' fails verification")]
    Corrupt {
        /// The content hash under verifying.
        sha: Sha,
    },
    /// Failed compression holding no further detail.
    #[error("cannot compress blob")]
    Compress,
}

impl BlobError {
    /// Maps one read io failure at the content hash into domain language.
    pub fn from_read_io(sha: &Sha, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match AccessFault::interpret(kind) {
            Some(fault) => Self::Read {
                sha: sha.clone(),
                fault,
            },
            None => Self::Read {
                sha: sha.clone(),
                fault: AccessFault::Unknown { message },
            },
        }
    }

    /// Maps one write io failure at the destination path into domain language.
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

/// Blob result alias.
pub type Result<T> = std::result::Result<T, BlobError>;
