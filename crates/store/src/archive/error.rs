//! Archive failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// Archive failure shapes.
#[derive(Debug, Error)]
pub enum ArchiveError {
    /// Missing source holding the path.
    #[error("cannot read '{path}': missing file", path = path.display())]
    Missing {
        /// Holds the source path under reading.
        path: PathBuf,
    },
    /// Denied source holding the path.
    #[error("cannot read '{path}': permission denied", path = path.display())]
    Denied {
        /// Holds the source path under reading.
        path: PathBuf,
    },
    /// Unknown failure holding the path with message.
    #[error("cannot read '{path}': {message}", path = path.display())]
    Unknown {
        /// Holds the source path under reading.
        path: PathBuf,
        /// Holds the failure message under reading.
        message: String,
    },
    /// Non-archive bytes holding the source path.
    #[error("cannot archive '{path}': not a compressed archive", path = path.display())]
    NotArchive {
        /// Holds the source path under sealing.
        path: PathBuf,
    },
    /// Escaping member holding the offending member path.
    #[error("cannot unpack '{path}': member escapes", path = path.display())]
    Escape {
        /// Holds the offending member path.
        path: PathBuf,
    },
    /// Unknown member holding the archive path with the member name.
    #[error("cannot unpack '{path}': unknown member '{name}'", path = path.display())]
    UnknownMember {
        /// Holds the archive path under unpacking.
        path: PathBuf,
        /// Holds the wanted member name.
        name: String,
    },
    /// Corrupt archive holding the source path.
    #[error("cannot unpack '{path}': corrupt archive", path = path.display())]
    CorruptedArchive {
        /// Holds the source path under unpacking.
        path: PathBuf,
    },
    /// Locked archive holding the source path.
    #[error("cannot unpack '{path}': password protected", path = path.display())]
    PasswordProtectedArchive {
        /// Holds the source path under unpacking.
        path: PathBuf,
    },
    /// Sealed compression holding the source path.
    #[error("cannot unpack '{path}': unsupported compression", path = path.display())]
    UnsupportedCompression {
        /// Holds the source path under unpacking.
        path: PathBuf,
    },
    /// Failed write holding the archive path with the fault.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// Holds the archive path under spilling.
        path: PathBuf,
        /// Holds the write fault cause.
        fault: AccessFault,
    },
}

impl ArchiveError {
    /// Maps one io failure at the archive path into domain language.
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

    /// Maps one spill io failure at the archive path into domain language.
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

/// Archive result alias.
pub type Result<T> = std::result::Result<T, ArchiveError>;

#[cfg(test)]
mod tests {
    use super::*;

    fn write_io(path: &Path, kind: std::io::ErrorKind) -> ArchiveError {
        ArchiveError::from_write_io(path, std::io::Error::new(kind, "disk failed"))
    }

    #[test]
    fn write_carries_archive_with_fault() {
        let path = Path::new("fonts.tar.gz");
        let error = write_io(path, std::io::ErrorKind::QuotaExceeded);
        match &error {
            ArchiveError::Write { path: found, fault } => {
                assert_eq!(found, path, "write keeps the path");
                assert!(
                    matches!(*fault, AccessFault::QuotaExceeded),
                    "write keeps the fault: {error}"
                );
            }
            other => panic!("wrong write variant: {other}"),
        }
    }
}
