//! Resource failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// Resource failure shapes.
#[derive(Debug, Clone, Error)]
pub enum ResourceError {
    /// Failed read holding the resource path with the fault.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// Holds the resource path under reading.
        path: PathBuf,
        /// Holds the read fault cause.
        fault: AccessFault,
    },
    /// Escaping path holding the offending path.
    #[error("resource path '{path}' escapes exec root", path = path.display())]
    Escape {
        /// Holds the offending resource path.
        path: PathBuf,
    },
}

impl ResourceError {
    /// Maps one io failure at the resource path into domain language.
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
}

/// Resource result alias.
pub type Result<T> = std::result::Result<T, ResourceError>;
