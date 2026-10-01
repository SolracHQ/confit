//! Faults
//!
//! Shared write fault causes across stores.

use std::fmt::{Display, Formatter};
use std::io::ErrorKind;

/// Shared write fault cause.
///
/// One cause covers every store write path. Stores pair the
/// cause with their own context in one Write wrapper. Reads
/// keep their own Missing with Denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessFault {
    /// Vanished destination.
    Missing,
    /// Refused destination.
    Denied,
    /// Full disk.
    StorageFull,
    /// Sealed disk.
    ReadOnlyFilesystem,
    /// Capped disk.
    QuotaExceeded,
    /// Oversized file.
    FileTooLarge,
}

impl AccessFault {
    /// Maps one write io kind into its fault.
    ///
    /// Callers send the six write kinds alone.
    pub fn from_kind(kind: ErrorKind) -> Self {
        match kind {
            ErrorKind::NotFound => Self::Missing,
            ErrorKind::PermissionDenied => Self::Denied,
            ErrorKind::StorageFull => Self::StorageFull,
            ErrorKind::ReadOnlyFilesystem => Self::ReadOnlyFilesystem,
            ErrorKind::QuotaExceeded => Self::QuotaExceeded,
            ErrorKind::FileTooLarge => Self::FileTooLarge,
            _ => unreachable!("write paths send the six kinds alone"),
        }
    }

    /// Names the fault cause for display shells.
    ///
    /// The cause reads as a bare phrase for appending.
    pub fn cause(&self) -> &'static str {
        match self {
            Self::Missing => "missing file",
            Self::Denied => "permission denied",
            Self::StorageFull => "storage full",
            Self::ReadOnlyFilesystem => "read-only filesystem",
            Self::QuotaExceeded => "quota exceeded",
            Self::FileTooLarge => "file too large",
        }
    }
}

impl Display for AccessFault {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.cause())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_table_names_six_faults() {
        let table = [
            (ErrorKind::NotFound, AccessFault::Missing),
            (ErrorKind::PermissionDenied, AccessFault::Denied),
            (ErrorKind::StorageFull, AccessFault::StorageFull),
            (
                ErrorKind::ReadOnlyFilesystem,
                AccessFault::ReadOnlyFilesystem,
            ),
            (ErrorKind::QuotaExceeded, AccessFault::QuotaExceeded),
            (ErrorKind::FileTooLarge, AccessFault::FileTooLarge),
        ];
        for (kind, want) in table {
            let error = std::io::Error::new(kind, "disk failed");
            assert_eq!(
                AccessFault::from_kind(error.kind()),
                want,
                "kind keeps its fault"
            );
            assert!(!want.cause().is_empty(), "fault cause renders: {want:?}");
        }
    }
}
