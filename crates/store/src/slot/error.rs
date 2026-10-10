//! Slot failures.

use std::path::{Path, PathBuf};

use confit_model::error::Error as ModelError;
use thiserror::Error;

use crate::faults::AccessFault;

/// Slot failure shapes.
#[derive(Debug, Error)]
pub enum SlotError {
    /// Failed read.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// The slot path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Corrupt state.
    #[error("cannot read '{path}': corrupt state", path = path.display())]
    Corrupt {
        /// The slot path under reading.
        path: PathBuf,
    },
    /// Unsupported version.
    #[error("state version {got} is unsupported")]
    Version {
        /// The slot path under reading.
        path: PathBuf,
        /// The seen version.
        got: u64,
    },
    /// Bad pick.
    #[error("bad slot pick '{input}'")]
    BadPick {
        /// The picker input under resolving.
        input: String,
    },
    /// Failed write.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The slot path under writing.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Failed manifest render.
    #[error("cannot write '{path}': cannot render manifest: {source}", path = path.display())]
    Manifest {
        /// The slot path under storing.
        path: PathBuf,
        /// The model render cause.
        source: ModelError,
    },
    /// Unknown write failure.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// The slot path under writing.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
}

impl SlotError {
    /// Map one io failure at the slot path into domain language.
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

    /// Map one write io failure at the slot path into domain language.
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

/// Slot result alias.
pub type Result<T> = std::result::Result<T, SlotError>;
