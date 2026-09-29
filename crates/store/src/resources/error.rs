//! Resource failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

/// Resource failure shapes.
#[derive(Debug, Error)]
pub enum ResourceError {
    /// Missing file holding the path.
    #[error("cannot read '{path}': missing file", path = path.display())]
    Missing {
        /// Holds the resource path under reading.
        path: PathBuf,
    },
    /// Escaping path holding the offending path.
    #[error("resource path '{path}' escapes exec root", path = path.display())]
    Escape {
        /// Holds the offending resource path.
        path: PathBuf,
    },
    /// Denied file holding the path.
    #[error("cannot read '{path}': permission denied", path = path.display())]
    Denied {
        /// Holds the resource path under reading.
        path: PathBuf,
    },
    /// Unknown failure holding the path with message.
    #[error("cannot read '{path}': {message}", path = path.display())]
    Unknown {
        /// Holds the resource path under reading.
        path: PathBuf,
        /// Holds the failure message under reading.
        message: String,
    },
}

impl ResourceError {
    /// Maps one io failure at the resource path into domain language.
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

/// Resource result alias.
pub type Result<T> = std::result::Result<T, ResourceError>;
