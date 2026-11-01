//! Reads zip members.

use std::io;

use confit_core::error::ContextualResult;

use super::zip_error;

/// Collects the zip member names in archive order.
///
/// # Errors
///
/// - [`confit_core::error::ArchiveError::PasswordLocked`] when the archive is locked.
/// - [`confit_core::error::ArchiveError::AlgorithmUnknown`] when the archive uses an unknown feature.
/// - [`confit_core::error::ArchiveError::BackendCorrupt`] when the archive is corrupt.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
pub(super) fn names<R: io::Read + io::Seek>(reader: R) -> ContextualResult<Vec<String>> {
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
        let name = entry.name().map_err(zip_error)?.into_owned();
        if name.is_empty() {
            continue;
        }
        found.push(name);
    }
    Ok(found)
}

/// Calls the visitor for each zip member.
///
/// # Errors
///
/// - [`confit_core::error::ArchiveError::PasswordLocked`] when the archive is locked.
/// - [`confit_core::error::ArchiveError::AlgorithmUnknown`] when the archive uses an unknown feature.
/// - [`confit_core::error::ArchiveError::BackendCorrupt`] when the archive is corrupt or an entry is absent.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
pub(super) fn walk<R: io::Read + io::Seek>(
    reader: R,
    mut visit: impl FnMut(super::Entry<'_>) -> ContextualResult<()>,
) -> ContextualResult<()> {
    let mut archive = zip::ZipArchive::new(reader).map_err(zip_error)?;
    let total = archive.len();
    let mut index = 0;
    while index < total {
        let entry = archive.by_index(index).map_err(zip_error)?;
        index += 1;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().map_err(zip_error)?.into_owned();
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
