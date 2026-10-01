//! Slot failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// Slot failure shapes.
#[derive(Debug, Error)]
pub enum SlotError {
    /// Failed read holding the slot path with the fault.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// Holds the slot path under reading.
        path: PathBuf,
        /// Holds the read fault cause.
        fault: AccessFault,
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
    /// Unknown write failure holding the slot path with message.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// Holds the slot path under writing.
        path: PathBuf,
        /// Holds the failure message under writing.
        message: String,
    },
}

impl SlotError {
    /// Maps one io failure at the slot path into domain language.
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
            _ => Self::WriteUnknown {
                path: path.to_path_buf(),
                message,
            },
        }
    }
}

/// Slot result alias.
pub type Result<T> = std::result::Result<T, SlotError>;
