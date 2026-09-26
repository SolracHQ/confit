//! Zip backend
//!
//! Seekable member listing and extraction for zip archives.

use std::path::Path;

use confit_model::error::{Error, Result};

use super::{ArchiveBackend, BornMember, spill_entry, unpack_failure};
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
                .map_err(|error| unpack_failure(source, error))?;
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
                .map_err(|error| unpack_failure(source, error))?;
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
/// Folders plus links skip as absent downstream.
///
/// # Errors
///
/// Missing sources fail as plan errors naming the
/// source. Undecodable archives fail as plan errors
/// naming the source.
fn open_archive(source: &Path) -> Result<zip::ZipArchive<Box<dyn driver::SeekRead>>> {
    let seekable = driver::open_seek(source)
        .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
    zip::ZipArchive::new(seekable).map_err(|error| unpack_failure(source, error))
}
