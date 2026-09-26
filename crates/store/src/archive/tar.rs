//! Tar backends
//!
//! Streaming member listing and extraction for plain
//! and gzipped tar archives.

use std::path::Path;

use confit_model::error::{Error, Result};

use super::{ArchiveBackend, BornMember, spill_entry, unpack_failure};
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
/// Missing and unreadable sources fail as plan errors
/// naming the source.
fn open_source(source: &Path) -> Result<Box<dyn std::io::Read>> {
    driver::open_read(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))
}

/// Lists file member names from a tar stream without keeping content.
///
/// Entry bytes stream to a sink, so listings hold names only.
///
/// # Errors
///
/// Malformed archives fail as plan errors naming the archive.
fn stream_names<R: std::io::Read>(reader: R, archive: &Path) -> Result<Vec<String>> {
    let mut reader = tar::Archive::new(reader);
    let entries = reader
        .entries()
        .map_err(|error| unpack_failure(archive, error))?;
    let mut names = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| unpack_failure(archive, error))?;
        if let Some(name) = entry_name(&entry, archive)? {
            names.push(name);
        }
        std::io::copy(&mut entry, &mut std::io::sink())
            .map_err(|error| unpack_failure(archive, error))?;
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
/// Decoder and spill failures surface as plan errors naming
/// the source.
fn unpack_tar_entries<R: std::io::Read>(
    reader: R,
    source: &Path,
    staging: &Path,
) -> Result<Vec<BornMember>> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| unpack_failure(source, error))?;
    let mut born = Vec::new();
    for entry in entries {
        let mut entry = entry.map_err(|error| unpack_failure(source, error))?;
        let Some(name) = entry_name(&entry, source)? else {
            std::io::copy(&mut entry, &mut std::io::sink())
                .map_err(|error| unpack_failure(source, error))?;
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
/// Undecodable entry paths fail as plan errors naming the
/// archive.
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
        .map_err(|error| unpack_failure(archive, error))?
        .to_string_lossy()
        .into_owned();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}
