//! Tar backend
//!
//! Streaming member listing and extraction for plain
//! and gzipped tar archives over driver framing.

use std::path::Path;

use super::error::{ArchiveError, Result};
use super::spill::{BornMember, from_stream, spill_entry};
use super::{ArchiveBackend, wants_tar};
use confit_driver as driver;

/// Streaming tar member listing and extraction.
///
/// Driver framing sniffs gzip itself, so plain with
/// gzipped members ride one verb. Lone singles read
/// under the archive file name.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TarBackend;

impl ArchiveBackend for TarBackend {
    fn names(&self, source: &Path) -> Result<Vec<String>> {
        let reader = open_source(source)?;
        let fallback = single_fallback(source);
        driver::tar::list_names(reader, &fallback).map_err(|error| from_stream(source, error))
    }

    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
        unpack_stream(open_source(source)?, source, staging)
    }
}

/// Opens one archive source for streamed reading.
///
/// # Errors
///
/// - [`ArchiveError::Read`] for failed source reads.
fn open_source(source: &Path) -> Result<Box<dyn std::io::Read>> {
    let reader = driver::fs::open(source);
    reader
        .map(|file| file as Box<dyn std::io::Read>)
        .map_err(|error| ArchiveError::from_io(source, error))
}

/// Unpacks tar entries with per-entry streaming hashes.
///
/// Skipped entries drain inside framing, kept
/// entries spill under staging with hashes. Lone
/// singles read under the archive file name.
///
/// # Errors
///
/// - [`ArchiveError::Escape`] for escaping members.
/// - [`ArchiveError::Read`] for failed archive reads.
/// - [`ArchiveError::CorruptedArchive`] for broken archives.
fn unpack_stream(
    reader: Box<dyn std::io::Read>,
    source: &Path,
    staging: &Path,
) -> Result<Vec<BornMember>> {
    let mut born = Vec::new();
    let mut failure: Option<ArchiveError> = None;
    let fallback = single_fallback(source);
    let outcome = driver::tar::walk(reader, &fallback, |framed| {
        let driver::tar::Member { name, reader } = framed;
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
                Err(from_stream(source, error))
            }
        }
    }
}

/// Derives the lone single fallback from an archive path.
///
/// Tar-style names refuse singles with an empty fallback,
/// so bare gzip under tar names fails loud as non-archives.
fn single_fallback(source: &Path) -> String {
    if wants_tar(source) {
        String::new()
    } else {
        member_name(source)
    }
}

/// Derives the member name from an archive path.
fn member_name(archive: &Path) -> String {
    let base = archive
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    if base.to_lowercase().ends_with(".gz") && base.len() > 3 {
        base[..base.len() - 3].to_string()
    } else if base.is_empty() {
        "file".to_string()
    } else {
        base
    }
}
