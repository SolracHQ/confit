//! Zip backend
//!
//! Seekable member listing and extraction for zip archives.

use std::path::Path;

use super::error::{ArchiveError, Result};
use super::{ArchiveBackend, BornMember, spill_entry};
use confit_driver as driver;

/// Seekable zip member listing and extraction.
///
/// Central-directory reads serve names, entry streams
/// spill members one at a time.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ZipBackend;

impl ArchiveBackend for ZipBackend {
    fn names(&self, source: &Path) -> Result<Vec<String>> {
        let mut archive = open_archive(source)?;
        let mut names = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| from_zip(source, error))?;
            if !entry.is_file() {
                continue;
            }
            let name = entry.name().to_owned();
            if name.is_empty() {
                continue;
            }
            names.push(name);
        }
        Ok(names)
    }

    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
        let mut archive = open_archive(source)?;
        let mut born = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            let entry = archive
                .by_index(index)
                .map_err(|error| from_zip(source, error))?;
            if !entry.is_file() {
                continue;
            }
            let name = entry.name().to_owned();
            if name.is_empty() {
                continue;
            }
            born.push(spill_entry(source, staging, &name, entry)?);
        }
        Ok(born)
    }
}

/// Opens one seekable zip archive without loading bytes.
///
/// Folders and links skip as absent downstream.
///
/// # Errors
///
/// - [`ArchiveError::Missing`] for missing sources.
/// - [`ArchiveError::Denied`] for denied sources.
/// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
/// - [`ArchiveError::PasswordProtectedArchive`] for locked archives.
/// - [`ArchiveError::UnsupportedCompression`] for sealed compression.
/// - [`ArchiveError::Unknown`] for other failures.
fn open_archive(source: &Path) -> Result<zip::ZipArchive<Box<dyn driver::SeekRead>>> {
    let seekable =
        driver::open_seek(source).map_err(|error| ArchiveError::from_io(source, error))?;
    zip::ZipArchive::new(seekable).map_err(|error| from_zip(source, error))
}

/// Maps one zip failure into its archive error.
fn from_zip(source: &Path, error: zip::result::ZipError) -> ArchiveError {
    match error {
        zip::result::ZipError::FileNotFound | zip::result::ZipError::InvalidArchive(_) => {
            ArchiveError::CorruptedArchive {
                path: source.to_path_buf(),
            }
        }
        zip::result::ZipError::InvalidPassword => ArchiveError::PasswordProtectedArchive {
            path: source.to_path_buf(),
        },
        zip::result::ZipError::CompressionMethodNotSupported(_) => {
            ArchiveError::UnsupportedCompression {
                path: source.to_path_buf(),
            }
        }
        zip::result::ZipError::UnsupportedArchive(message)
            if message == zip::result::ZipError::PASSWORD_REQUIRED =>
        {
            ArchiveError::PasswordProtectedArchive {
                path: source.to_path_buf(),
            }
        }
        zip::result::ZipError::UnsupportedArchive(
            "Seekable compressed files are not yet supported",
        ) => ArchiveError::UnsupportedCompression {
            path: source.to_path_buf(),
        },
        zip::result::ZipError::UnsupportedArchive(message) => ArchiveError::Unknown {
            path: source.to_path_buf(),
            message: message.to_string(),
        },
        zip::result::ZipError::Io(error) => ArchiveError::from_io(source, error),
        other => ArchiveError::Unknown {
            path: source.to_path_buf(),
            message: other.to_string(),
        },
    }
}
