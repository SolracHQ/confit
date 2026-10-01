//! Slot failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// Slot failure shapes.
#[derive(Debug, Error)]
pub enum SlotError {
    /// Missing slot holding the path.
    #[error("cannot read '{path}': missing file", path = path.display())]
    Missing {
        /// Holds the slot path under reading.
        path: PathBuf,
    },
    /// Denied slot holding the path.
    #[error("cannot read '{path}': permission denied", path = path.display())]
    Denied {
        /// Holds the slot path under reading.
        path: PathBuf,
    },
    /// Unknown failure holding the path with message.
    #[error("cannot read '{path}': {message}", path = path.display())]
    Unknown {
        /// Holds the slot path under reading.
        path: PathBuf,
        /// Holds the failure message under reading.
        message: String,
    },
    /// Corrupt state holding the path.
    #[error("cannot read '{path}': corrupt state", path = path.display())]
    Corrupt {
        /// Holds the slot path under reading.
        path: PathBuf,
    },
    /// Unsupported version holding the path with the seen version.
    #[error("state version {got} reads unsupported")]
    Version {
        /// Holds the slot path under reading.
        path: PathBuf,
        /// Holds the seen version.
        got: u64,
    },
    /// Bad pick holding the picker input.
    #[error("bad slot pick '{input}'")]
    BadPick {
        /// Holds the picker input under resolving.
        input: String,
    },
    /// Failed write holding the slot path with the fault.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// Holds the slot path under writing.
        path: PathBuf,
        /// Holds the write fault cause.
        fault: AccessFault,
    },
}

impl SlotError {
    /// Maps one io failure at the slot path into domain language.
    pub fn from_io(path: &Path, error: std::io::Error) -> Self {
        let kind = error.kind();
        let message = error.to_string();
        match kind {
            std::io::ErrorKind::NotFound => Self::Missing {
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

    /// Maps one write io failure at the slot path into domain language.
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

/// Slot result alias.
pub type Result<T> = std::result::Result<T, SlotError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn write_io(path: &Path, kind: std::io::ErrorKind) -> SlotError {
        SlotError::from_write_io(path, std::io::Error::new(kind, "disk failed"))
    }

    #[test]
    fn write_carries_slot_with_fault() {
        let path = Path::new("state.json");
        let error = write_io(path, std::io::ErrorKind::ReadOnlyFilesystem);
        match &error {
            SlotError::Write { path: found, fault } => {
                assert_eq!(found, path, "write keeps the path");
                assert!(
                    matches!(*fault, AccessFault::ReadOnlyFilesystem),
                    "write keeps the fault: {error}"
                );
            }
            other => panic!("wrong write variant: {other}"),
        }
    }
}
