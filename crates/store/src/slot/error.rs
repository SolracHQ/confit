//! Slot failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

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
}

/// Slot result alias.
pub type Result<T> = std::result::Result<T, SlotError>;
