//! Blob failures.

use std::path::{Path, PathBuf};

use confit_model::sha::Sha;
use thiserror::Error;

/// Blob failure shapes.
#[derive(Debug, Error)]
pub enum BlobError {
    /// Missing pool entry holding the content hash.
    #[error("missing blob '{sha}'")]
    Missing {
        /// Holds the content hash under reading.
        sha: Sha,
    },
    /// Denied pool entry holding the content hash.
    #[error("cannot read blob '{sha}': permission denied")]
    Denied {
        /// Holds the content hash under reading.
        sha: Sha,
    },
    /// Unknown failure holding the content hash with message.
    #[error("cannot read blob '{sha}': {message}")]
    Unknown {
        /// Holds the content hash under reading.
        sha: Sha,
        /// Holds the failure message under reading.
        message: String,
    },
    /// Missing destination holding the path.
    #[error("cannot write '{path}': missing file", path = path.display())]
    WriteMissing {
        /// Holds the destination path under writing.
        path: PathBuf,
    },
    /// Denied destination holding the path.
    #[error("cannot write '{path}': permission denied", path = path.display())]
    WriteDenied {
        /// Holds the destination path under writing.
        path: PathBuf,
    },
    /// Unknown write failure holding the path with message.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// Holds the destination path under writing.
        path: PathBuf,
        /// Holds the failure message under writing.
        message: String,
    },
    /// Corrupt pool entry holding the content hash.
    #[error("blob '{sha}' fails verification")]
    Corrupt {
        /// Holds the content hash under verifying.
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
        match kind {
            std::io::ErrorKind::NotFound => Self::Missing { sha: sha.clone() },
            std::io::ErrorKind::PermissionDenied => Self::Denied { sha: sha.clone() },
            _ => Self::Unknown {
                sha: sha.clone(),
                message,
            },
        }
    }

    /// Maps one write io failure at the destination path into domain language.
    pub fn from_write_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match kind {
            std::io::ErrorKind::NotFound => Self::WriteMissing {
                path: path.to_path_buf(),
            },
            std::io::ErrorKind::PermissionDenied => Self::WriteDenied {
                path: path.to_path_buf(),
            },
            _ => Self::WriteUnknown {
                path: path.to_path_buf(),
                message,
            },
        }
    }
}

/// Blob result alias.
pub type Result<T> = std::result::Result<T, BlobError>;
