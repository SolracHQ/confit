//! Tar backends
//!
//! Streaming member listing and extraction for plain
//! and gzipped tar archives.

use std::path::Path;

use super::error::{ArchiveError, Result};
use super::{ArchiveBackend, BornMember, from_stream, spill_entry};
use confit_driver as driver;

/// Streaming plain-tar member listing and extraction.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TarBackend;

/// Streaming gzipped-tar member listing and extraction.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GzippedTarBackend;

impl ArchiveBackend for TarBackend {
    fn names(&self, source: &Path) -> Result<Vec<String>> {
        stream_names(open_source(source)?, source)
    }

    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
        unpack_tar_entries(open_source(source)?, source, staging)
    }
}

impl ArchiveBackend for GzippedTarBackend {
    fn names(&self, source: &Path) -> Result<Vec<String>> {
        stream_names(flate2::read::GzDecoder::new(open_source(source)?), source)
    }

    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
        unpack_tar_entries(
            flate2::read::GzDecoder::new(open_source(source)?),
            source,
            staging,
        )
    }
}

/// Opens one archive source for streamed reading.
///
/// # Errors
///
/// - [`ArchiveError::Missing`] for missing sources.
/// - [`ArchiveError::Denied`] for denied sources.
/// - [`ArchiveError::Unknown`] for other failures.
fn open_source(source: &Path) -> Result<Box<dyn std::io::Read>> {
    driver::open_read(source).map_err(|error| ArchiveError::from_io(source, error))
}

/// Lists file member names from a tar stream without keeping content.
///
/// Entry bytes stream to a sink, so listings hold names only.
///
/// # Errors
///
/// - [`ArchiveError::Missing`] for missing archives.
/// - [`ArchiveError::Denied`] for denied archives.
/// - [`ArchiveError::Unknown`] for other stream failures.
/// - [`ArchiveError::CorruptedArchive`] for broken archives.
fn stream_names<R: std::io::Read>(reader: R, archive: &Path) -> Result<Vec<String>> {
    let mut reader = tar::Archive::new(reader);
    let entries = reader
        .entries()
        .map_err(|_| ArchiveError::CorruptedArchive {
            path: archive.to_path_buf(),
        })?;
    let mut names = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| from_stream(archive, error))?;
        if let Some(name) = entry_name(&entry, archive)? {
            names.push(name);
        }
        std::io::copy(&mut entry, &mut std::io::sink())
            .map_err(|error| from_stream(archive, error))?;
    }
    Ok(names)
}

/// Unpacks tar entries with per-entry streaming hashes.
///
/// Skipped entries drain to a sink. Chunk copies feed
/// the member hash.
///
/// # Errors
///
/// - [`ArchiveError::Missing`] for missing archives.
/// - [`ArchiveError::Denied`] for denied archives.
/// - [`ArchiveError::Unknown`] for other stream failures.
/// - [`ArchiveError::CorruptedArchive`] for broken archives.
fn unpack_tar_entries<R: std::io::Read>(
    reader: R,
    source: &Path,
    staging: &Path,
) -> Result<Vec<BornMember>> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|_| ArchiveError::CorruptedArchive {
            path: source.to_path_buf(),
        })?;
    let mut born = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| from_stream(source, error))?;
        let Some(name) = entry_name(&entry, source)? else {
            std::io::copy(&mut entry, &mut std::io::sink())
                .map_err(|error| from_stream(source, error))?;
            continue;
        };
        born.push(spill_entry(source, staging, &name, entry)?);
    }
    Ok(born)
}

/// Reads the file name for one tar entry.
///
/// Folders, non-files, and empty names skip as absent.
///
/// # Errors
///
/// - [`ArchiveError::CorruptedArchive`] for undecodable entry paths.
fn entry_name<R: std::io::Read>(
    entry: &tar::Entry<'_, R>,
    archive: &Path,
) -> Result<Option<String>> {
    let kind = entry.header().entry_type();
    if kind.is_dir() || !kind.is_file() {
        return Ok(None);
    }
    let name = entry
        .path()
        .map_err(|_| ArchiveError::CorruptedArchive {
            path: archive.to_path_buf(),
        })?
        .to_string_lossy()
        .into_owned();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}
