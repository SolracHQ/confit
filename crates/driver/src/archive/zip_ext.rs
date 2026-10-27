//! Zip member reading.

use std::io;

use confit_core::error::Result as CoreResult;

use super::{Result, zip_error};

/// Return zip member names in archive order.
///
/// # Errors
///
/// - [`confit_core::error::ArchiveError::PasswordLocked`] for locked archives.
/// - [`confit_core::error::ArchiveError::AlgorithmUnknown`] for unknown features and methods.
/// - [`confit_core::error::ArchiveError::BackendCorrupt`] for corrupt archives.
/// - [`confit_core::error::FilesystemError`] for denied access and missing input.
pub(super) fn names<R: io::Read + io::Seek>(reader: R) -> Result<Vec<String>> {
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
    let total = archive.len();
    let mut found = Vec::with_capacity(total);
    let mut index = 0;
    while index < total {
        let entry = archive.by_index(index).map_err(zip_error)?;
        index += 1;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_owned();
        if name.is_empty() {
            continue;
        }
        found.push(name);
    }
    Ok(found)
}

/// Walk zip members with one visitor.
///
/// # Errors
///
/// - [`confit_core::error::ArchiveError::PasswordLocked`] for locked archives.
/// - [`confit_core::error::ArchiveError::AlgorithmUnknown`] for unknown features and methods.
/// - [`confit_core::error::ArchiveError::BackendCorrupt`] for corrupt archives and missing entries.
/// - [`confit_core::error::FilesystemError`] for denied access and missing input.
pub(super) fn walk<R: io::Read + io::Seek>(
    reader: R,
    mut visit: impl FnMut(super::Entry<'_>) -> CoreResult<()>,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
    let total = archive.len();
    let mut index = 0;
    while index < total {
        let entry = archive.by_index(index).map_err(zip_error)?;
        index += 1;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_owned();
        if name.is_empty() {
            continue;
        }
        let mode = entry
            .unix_mode()
            .map(|mode| mode & super::MODE_MASK)
            .unwrap_or(super::FALLBACK_MODE);
        let member = super::Entry {
            name,
            mode,
            reader: Box::new(entry),
        };
        visit(member)?;
    }
    Ok(())
}
