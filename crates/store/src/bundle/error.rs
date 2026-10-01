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
    /// Unreachable bundle holding the path.
    #[error("bundle '{path}': missing file", path = path.display())]
    Unreachable {
        /// Holds the bundle path under reading.
        path: PathBuf,
    },
    /// Denied bundle holding the path.
    #[error("bundle '{path}': permission denied", path = path.display())]
    Denied {
        /// Holds the bundle path under reading.
        path: PathBuf,
    },
    /// Unknown failure holding the bundle path with message.
    #[error("bundle '{path}': {message}", path = path.display())]
    Unknown {
        /// Holds the bundle path under reading.
        path: PathBuf,
        /// Holds the failure message under reading.
        message: String,
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
}

impl BundleError {
    /// Maps one io failure at the bundle path into domain language.
    pub fn from_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match kind {
            std::io::ErrorKind::NotFound => Self::Unreachable {
                path: path.to_path_buf(),
            },
            std::io::ErrorKind::PermissionDenied => Self::Denied {
                path: path.to_path_buf(),
            },
            _ => Self::Unknown {
                path: path.to_path_buf(),
                message,
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
            _ => Self::Unknown {
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
        ArchiveError::Missing { .. } => BundleError::Unreachable {
            path: bundle.to_path_buf(),
        },
        ArchiveError::Denied { .. } => BundleError::Denied {
            path: bundle.to_path_buf(),
        },
        ArchiveError::Write { fault, .. } => BundleError::Write {
            path: bundle.to_path_buf(),
            fault,
        },
        ArchiveError::Unknown { message, .. } => BundleError::Unknown {
            path: bundle.to_path_buf(),
            message,
        },
        other => BundleError::Unknown {
            path: bundle.to_path_buf(),
            message: other.to_string(),
        },
    }
}

/// Maps one resource failure at the bundle path into bundle language.
pub(super) fn from_resource(bundle: &Path, error: ResourceError) -> BundleError {
    match error {
        ResourceError::Missing { .. } => BundleError::Unreachable {
            path: bundle.to_path_buf(),
        },
        ResourceError::Denied { .. } => BundleError::Denied {
            path: bundle.to_path_buf(),
        },
        ResourceError::Unknown { message, .. } => BundleError::Unknown {
            path: bundle.to_path_buf(),
            message,
        },
        other => BundleError::Unknown {
            path: bundle.to_path_buf(),
            message: other.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_io(path: &Path, kind: std::io::ErrorKind) -> BundleError {
        BundleError::from_write_io(path, std::io::Error::new(kind, "disk failed"))
    }

    #[test]
    fn write_carries_bundle_with_fault() {
        let path = Path::new("plan.cb");
        let error = write_io(path, std::io::ErrorKind::StorageFull);
        match &error {
            BundleError::Write { path: found, fault } => {
                assert_eq!(found, path, "write keeps the path");
                assert!(
                    matches!(*fault, AccessFault::StorageFull),
                    "write keeps the fault: {error}"
                );
            }
            other => panic!("wrong write variant: {other}"),
        }
    }

    #[test]
    fn archive_write_kinds_forward_with_bundle() {
        let bundle = Path::new("plan.cb");
        let error = from_archive(
            bundle,
            ArchiveError::Write {
                path: bundle.to_path_buf(),
                fault: AccessFault::StorageFull,
            },
        );
        match &error {
            BundleError::Write { path: found, fault } => {
                assert_eq!(found, bundle, "forward keeps the path");
                assert!(
                    matches!(*fault, AccessFault::StorageFull),
                    "forward keeps the fault: {error}"
                );
            }
            other => panic!("wrong forward variant: {other}"),
        }
        let error = from_archive(
            bundle,
            ArchiveError::Write {
                path: bundle.to_path_buf(),
                fault: AccessFault::QuotaExceeded,
            },
        );
        assert!(
            matches!(
                error,
                BundleError::Write {
                    fault: AccessFault::QuotaExceeded,
                    ..
                }
            ),
            "capped spill forwards: {error}"
        );
    }
}
