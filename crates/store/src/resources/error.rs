//! Resource failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// Resource failure shapes.
#[derive(Debug, Clone, Error)]
pub enum ResourceError {
    /// Failed read.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// The resource path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Escaping path.
    #[error("resource path '{path}' escapes exec root: {reason}", path = path.display())]
    Escape {
        /// The offending resource path.
        path: PathBuf,
        /// The escape reason.
        reason: String,
    },
}

/// One segment offense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentFault {
    /// Empty segment string.
    Empty,
    /// Absolute path in relative position.
    Absolute,
    /// Parent step climbing out.
    Parent,
    /// Drive prefix showing a foreign host.
    Drive,
    /// Backslash separator from another platform.
    Backslash,
    /// Empty component from doubled or trailing slashes.
    EmptyComponent,
}

impl SegmentFault {
    /// Return the escape reason for the fault.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Empty => "expected a path",
            Self::Absolute => "expected a relative path",
            Self::Parent => "parent steps are not supported",
            Self::Drive => "drive prefixes are not supported",
            Self::Backslash => "backslashes are not supported",
            Self::EmptyComponent => "empty components are not supported",
        }
    }
}

impl std::fmt::Display for SegmentFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl ResourceError {
    /// Map one io failure at the resource path into domain language.
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
