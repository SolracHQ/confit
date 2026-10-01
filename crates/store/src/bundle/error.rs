//! Bundle failures.

use std::path::{Path, PathBuf};

use confit_model::sha::Sha;
use thiserror::Error;

use crate::archive::error::ArchiveError;
use crate::faults::AccessFault;
use crate::resources::error::ResourceError;

/// Bundle failure shapes.
#[derive(Debug, Error)]
pub enum BundleError {
    /// Failed read holding the bundle path with the fault.
    #[error("bundle '{path}': {fault}", path = path.display())]
    Read {
        /// Holds the bundle path under reading.
        path: PathBuf,
        /// Holds the read fault cause.
        fault: AccessFault,
    },
    /// Unsupported version holding the bundle path with the seen version.
    #[error("bundle version {got} reads unsupported")]
    Version {
        /// Holds the bundle path under reading.
        path: PathBuf,
        /// Holds the seen version.
        got: u64,
    },
    /// Missing blob holding the content hash.
    #[error("missing blob '{sha}'")]
    Missing {
        /// Holds the content hash under reading.
        sha: Sha,
    },
    /// Failed write holding the bundle path with the fault.
    #[error("bundle '{path}': {fault}", path = path.display())]
    Write {
        /// Holds the bundle path under writing.
        path: PathBuf,
        /// Holds the write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure holding the bundle path with message.
    #[error("bundle '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// Holds the bundle path under writing.
        path: PathBuf,
        /// Holds the failure message under writing.
        message: String,
    },
}

impl BundleError {
    /// Maps one io failure at the bundle path into domain language.
    pub fn from_io(path: &Path, error: std::io::Error) -> Self {
        let message = error.to_string();
        match error.kind() {
            kind @ (std::io::ErrorKind::NotFound
            | std::io::ErrorKind::PermissionDenied
            | std::io::ErrorKind::StorageFull
            | std::io::ErrorKind::ReadOnlyFilesystem
            | std::io::ErrorKind::QuotaExceeded
            | std::io::ErrorKind::FileTooLarge) => Self::Read {
                path: path.to_path_buf(),
                fault: AccessFault::from_kind(kind),
            },
            _ => Self::Read {
                path: path.to_path_buf(),
                fault: AccessFault::Unknown { message },
            },
        }
    }

    /// Maps one seal io failure at the bundle path into domain language.
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

/// Bundle result alias.
pub type Result<T> = std::result::Result<T, BundleError>;

/// Maps one archive failure at the bundle path into bundle language.
pub(super) fn from_archive(bundle: &Path, error: ArchiveError) -> BundleError {
    match error {
        ArchiveError::Read { fault, .. } => BundleError::Read {
            path: bundle.to_path_buf(),
            fault,
        },
        ArchiveError::Write { fault, .. } => BundleError::Write {
            path: bundle.to_path_buf(),
            fault,
        },
        ArchiveError::WriteUnknown { message, .. } => BundleError::WriteUnknown {
            path: bundle.to_path_buf(),
            message,
        },
        other => BundleError::Read {
            path: bundle.to_path_buf(),
            fault: AccessFault::Unknown {
                message: other.to_string(),
            },
        },
    }
}

/// Maps one resource failure at the bundle path into bundle language.
pub(super) fn from_resource(bundle: &Path, error: ResourceError) -> BundleError {
    match error {
        ResourceError::Read { fault, .. } => BundleError::Read {
            path: bundle.to_path_buf(),
            fault,
        },
        other => BundleError::Read {
            path: bundle.to_path_buf(),
            fault: AccessFault::Unknown {
                message: other.to_string(),
            },
        },
    }
}
