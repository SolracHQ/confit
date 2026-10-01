//! Blob failures.

use std::path::{Path, PathBuf};

use confit_model::sha::Sha;
use thiserror::Error;

use crate::faults::AccessFault;

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
    /// Failed write holding the destination path with the fault.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// Holds the destination path under writing.
        path: PathBuf,
        /// Holds the write fault cause.
        fault: AccessFault,
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
        let message = error.to_string();
        match error.kind() {
            kind @ (std::io::ErrorKind::NotFound
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::ReadOnlyFilesystem
            | std::io::ErrorKind::QuotaExceeded
            | std::io::ErrorKind::FileTooLarge) => Self::Write {
                path: path.to_path_buf(),
                fault: AccessFault::from_kind(kind),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_io(path: &Path, kind: std::io::ErrorKind) -> BlobError {
        BlobError::from_write_io(path, std::io::Error::new(kind, "disk failed"))
    }

    #[test]
    fn write_carries_destination_with_fault() {
        let path = Path::new("cache/blobs/stored");
        let error = write_io(path, std::io::ErrorKind::StorageFull);
        match &error {
            BlobError::Write { path: found, fault } => {
                assert_eq!(found, path, "write keeps the path");
                assert!(
                    matches!(*fault, AccessFault::StorageFull),
                    "write keeps the fault: {error}"
                );
            }
            other => panic!("wrong write variant: {other}"),
        }
    }
}
