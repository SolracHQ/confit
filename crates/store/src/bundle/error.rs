//! Bundle failures.

use std::path::{Path, PathBuf};

use confit_driver::sha::Sha;
use confit_model::error::Error as ModelError;
use thiserror::Error;

use crate::archive::error::ArchiveError;
use crate::faults::AccessFault;
use crate::resources::error::ResourceError;

/// Bundle failure shapes.
#[derive(Debug, Error)]
pub enum BundleError {
    /// Failed read.
    #[error("bundle '{path}': {fault}", path = path.display())]
    Read {
        /// The bundle path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Unsupported version.
    #[error("bundle version {got} is unsupported")]
    Version {
        /// The bundle path under reading.
        path: PathBuf,
        /// The seen version.
        got: u64,
    },
    /// Missing blob.
    #[error("missing blob '{sha}'")]
    Missing {
        /// The content hash under reading.
        sha: Sha,
    },
    /// Failed write.
    #[error("bundle '{path}': {fault}", path = path.display())]
    Write {
        /// The bundle path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Failed manifest render.
    #[error("bundle '{path}': cannot render manifest: {source}", path = path.display())]
    Manifest {
        /// The bundle path under sealing.
        path: PathBuf,
        /// The model render cause.
        source: ModelError,
    },
    /// Unknown write failure.
    #[error("bundle '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// The bundle path under writing.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
}

impl BundleError {
    /// Map one io failure at the bundle path into domain language.
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

    /// Map one seal io failure at the bundle path into domain language.
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

/// Bundle result alias.
pub type Result<T> = std::result::Result<T, BundleError>;

/// Map one archive failure at the bundle path into bundle language.
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

/// Map one resource failure at the bundle path into bundle language.
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
