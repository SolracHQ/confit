//! Zip member reading.
//!
//! Readers visit zip members under names and modes.

use std::io;

use super::Error;
use super::Result;

/// Return zip member names in archive order.
///
/// - Every returned name is a file with a non-empty name.
///
/// # Errors
///
/// - [`Error::Locked`] for locked archives.
/// - [`Error::Unknown`] for refused features and methods.
/// - [`Error::Corrupt`] for corrupt archives.
/// - [`Error::Denied`] for refused access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
pub(super) fn names<R: io::Read + io::Seek>(reader: R) -> Result<Vec<String>> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let total = archive.len();
    let mut found = Vec::with_capacity(total);
    let mut index = 0;
    while index < total {
        let entry = archive.by_index(index)?;
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
/// - Every visited member is a file with a non-empty name.
/// - Modes mask toward 0o777 with 0o644 fallback.
///
/// # Errors
///
/// - [`Error::Locked`] for locked archives.
/// - [`Error::Unknown`] for refused features and methods.
/// - [`Error::Corrupt`] for corrupt archives and missing entries.
/// - [`Error::Denied`] for refused access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
/// - [`Error::Visit`] for failing visitors.
pub(super) fn walk<R: io::Read + io::Seek>(
    reader: R,
    mut visit: impl FnMut(super::Entry<'_>) -> io::Result<()>,
) -> Result<()> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let total = archive.len();
    let mut index = 0;
    while index < total {
        let entry = archive.by_index(index)?;
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
        visit(member).map_err(Error::Visit)?;
    }
    Ok(())
}
