//! Archive failures.

use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::faults::AccessFault;

/// Archive failure shapes.
#[derive(Debug, Clone, Error)]
pub enum ArchiveError {
    /// Failed read.
    #[error("cannot read '{path}': {fault}", path = path.display())]
    Read {
        /// The source path under reading.
        path: PathBuf,
        /// The read fault cause.
        fault: AccessFault,
    },
    /// Non-archive bytes.
    #[error("cannot archive '{path}': not a compressed archive", path = path.display())]
    NotArchive {
        /// The source path under sealing.
        path: PathBuf,
    },
    /// Escaping member.
    #[error("cannot unpack '{path}': member escapes", path = path.display())]
    Escape {
        /// The offending member path.
        path: PathBuf,
    },
    /// Unknown member.
    #[error("cannot unpack '{path}': unknown member '{name}'", path = path.display())]
    UnknownMember {
        /// The archive path under unpacking.
        path: PathBuf,
        /// The wanted member name.
        name: String,
    },
    /// Corrupt archive.
    #[error("cannot unpack '{path}': corrupt archive", path = path.display())]
    CorruptedArchive {
        /// The source path under unpacking.
        path: PathBuf,
    },
    /// Locked archive.
    #[error("cannot unpack '{path}': password protected", path = path.display())]
    PasswordProtectedArchive {
        /// The source path under unpacking.
        path: PathBuf,
    },
    /// Sealed compression.
    #[error("cannot unpack '{path}': unsupported compression", path = path.display())]
    UnsupportedCompression {
        /// The source path under unpacking.
        path: PathBuf,
    },
    /// Failed write.
    #[error("cannot write '{path}': {fault}", path = path.display())]
    Write {
        /// The archive path under spilling.
        path: PathBuf,
        /// The write fault cause.
        fault: AccessFault,
    },
    /// Unknown write failure.
    #[error("cannot write '{path}': {message}", path = path.display())]
    WriteUnknown {
        /// The archive path under spilling.
        path: PathBuf,
        /// The failure message under writing.
        message: String,
    },
}

impl ArchiveError {
    /// Map one io failure at the archive path into domain language.
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

    /// Map one spill io failure at the archive path into domain language.
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

/// Archive result alias.
pub type Result<T> = std::result::Result<T, ArchiveError>;
