//! Faults
//!
//! Shared access fault causes across stores.

use std::fmt::{Display, Formatter};
use std::io::ErrorKind;

/// Shared access fault cause.
///
/// One cause covers every store access path. Stores pair the
/// cause with their own context in one Read wrapper and one
/// Write wrapper. Unmapped kinds carry their message along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessFault {
    /// Vanished file.
    Missing,
    /// Refused file.
    Denied,
    /// Full disk.
    StorageFull,
    /// Sealed disk.
    ReadOnlyFilesystem,
    /// Capped disk.
    QuotaExceeded,
    /// Oversized file.
    FileTooLarge,
    /// Unmapped failure carrying raw text.
    Unknown {
        /// Holds the raw failure text.
        message: String,
    },
}

impl AccessFault {
    /// Maps one io kind into its fault.
    ///
    /// Callers send the six mapped kinds alone.
    pub fn from_kind(kind: ErrorKind) -> Self {
        match kind {
            ErrorKind::NotFound => Self::Missing,
            ErrorKind::PermissionDenied => Self::Denied,
            ErrorKind::StorageFull => Self::StorageFull,
            ErrorKind::ReadOnlyFilesystem => Self::ReadOnlyFilesystem,
            ErrorKind::QuotaExceeded => Self::QuotaExceeded,
            ErrorKind::FileTooLarge => Self::FileTooLarge,
            _ => unreachable!("access paths send the six kinds alone"),
        }
    }

    /// Names the fault cause for display shells.
    ///
    /// The cause reads as a bare phrase for appending.
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
