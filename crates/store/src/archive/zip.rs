//! Zip backend
//!
//! Seekable member listing and extraction for zip archives.

use std::path::Path;

use super::ArchiveBackend;
use super::error::{ArchiveError, Result};
use super::spill::{BornMember, spill_entry};
use confit_driver as driver;

/// Seekable zip member listing and extraction.
///
/// Central-directory reads serve names, entry streams
/// spill members one at a time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ZipBackend;

impl ArchiveBackend for ZipBackend {
    fn names(&self, source: &Path) -> Result<Vec<String>> {
        let reader = open_source(source)?;
        driver::zip::list_names(reader).map_err(|error| from_zip(source, error))
    }

    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
        unpack_stream(open_source(source)?, source, staging)
    }
}

/// Opens one archive source for seekable reading.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed source reads.
fn open_source(source: &Path) -> Result<Box<dyn driver::fs::FsFile>> {
    let reader = driver::fs::open(source);
    reader.map_err(|error| ArchiveError::from_io(source, error))
}

/// Maps one zip failure into its archive error.
fn from_zip(source: &Path, error: driver::zip::ZipError) -> ArchiveError {
    match error {
        driver::zip::ZipError::FileNotFound | driver::zip::ZipError::InvalidArchive(_) => {
            ArchiveError::CorruptedArchive {
                path: source.to_path_buf(),
            }
        }
        driver::zip::ZipError::InvalidPassword => ArchiveError::PasswordProtectedArchive {
            path: source.to_path_buf(),
        },
        driver::zip::ZipError::CompressionMethodNotSupported(_) => {
            ArchiveError::UnsupportedCompression {
                path: source.to_path_buf(),
            }
        }
        driver::zip::ZipError::UnsupportedArchive(message)
            if message == driver::zip::ZipError::PASSWORD_REQUIRED =>
        {
            ArchiveError::PasswordProtectedArchive {
                path: source.to_path_buf(),
            }
        }
        driver::zip::ZipError::UnsupportedArchive(
            "Seekable compressed files are not yet supported",
        ) => ArchiveError::UnsupportedCompression {
            path: source.to_path_buf(),
        },
        driver::zip::ZipError::UnsupportedArchive(message) => {
            read_unknown(source, message.to_string())
        }
        driver::zip::ZipError::Io(error) => ArchiveError::from_io(source, error),
        other => read_unknown(source, other.to_string()),
    }
}

/// Builds one raw read failure at the source path.
fn read_unknown(source: &Path, message: String) -> ArchiveError {
    ArchiveError::Read {
        path: source.to_path_buf(),
        fault: crate::faults::AccessFault::Unknown { message },
    }
}
/// Unpacks zip entries with per-entry streaming hashes.
///
/// Skipped entries drain inside framing, kept
/// entries spill under staging with hashes.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for escaping members.
/// - [`ArchiveError::Read`] for failed archive reads.
/// - [`ArchiveError::CorruptedArchive`] for broken archives.
fn unpack_stream(
    reader: Box<dyn driver::fs::FsFile>,
    source: &Path,
    staging: &Path,
) -> Result<Vec<BornMember>> {
    let mut born = Vec::new();
    let mut failure: Option<ArchiveError> = None;
    let outcome = driver::zip::walk(reader, |framed| {
        let driver::zip::Member { name, reader } = framed;
        match spill_entry(source, staging, &name, reader) {
            Ok(member) => {
                born.push(member);
                Ok(())
            }
            Err(error) => {
                failure = Some(error);
                Err(std::io::Error::other("archive spill failed"))
            }
        }
    });
    match outcome {
        Ok(()) => Ok(born),
        Err(error) => {
            if let Some(error) = failure {
                Err(error)
            } else {
                Err(from_zip(source, error))
            }
        }
    }
}
