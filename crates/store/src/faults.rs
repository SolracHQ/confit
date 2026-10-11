//! Faults
//! Shared access fault causes across stores.

use std::fmt::{Display, Formatter};
use std::io::ErrorKind;

/// Shared access fault cause.
///
/// One cause covers every store access path. Stores pair the cause with their
/// own context in one Read wrapper and one Write wrapper. Unmapped kinds carry
/// their message along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessFault {
    /// Vanished file.
    Missing,
    /// Refused file.
    Denied,
    /// Full disk.
    StorageFull,
    /// Read-only disk.
    ReadOnlyFilesystem,
    /// Capped disk.
    QuotaExceeded,
    /// Oversized file.
    FileTooLarge,
    /// Unmapped failure with raw text.
    Unknown {
        /// The raw failure text.
        message: String,
    },
}

impl AccessFault {
    /// Interpret one io kind into its fault.
    pub fn interpret(kind: ErrorKind) -> Option<Self> {
        match kind {
            ErrorKind::NotFound => Some(Self::Missing),
            ErrorKind::PermissionDenied => Some(Self::Denied),
            ErrorKind::StorageFull => Some(Self::StorageFull),
            ErrorKind::ReadOnlyFilesystem => Some(Self::ReadOnlyFilesystem),
            ErrorKind::QuotaExceeded => Some(Self::QuotaExceeded),
            ErrorKind::FileTooLarge => Some(Self::FileTooLarge),
            _ => None,
        }
    }

    /// Return the fault cause for display shells.
    ///
    /// Unknown faults echo their raw text.
    pub fn cause(&self) -> &str {
        match self {
            Self::Missing => "missing file",
            Self::Denied => "permission denied",
            Self::StorageFull => "storage full",
            Self::ReadOnlyFilesystem => "read-only filesystem",
            Self::QuotaExceeded => "quota exceeded",
            Self::FileTooLarge => "file too large",
            Self::Unknown { message } => message,
        }
    }
}

impl Display for AccessFault {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.cause())
    }
}
