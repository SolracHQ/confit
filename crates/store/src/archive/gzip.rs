//! Gzip backend
//!
//! Streaming member listing and extraction for one
//! gzipped file outside any tar container.

use std::io::Read as _;
use std::path::Path;

use confit_model::error::{Error, Result};

use super::{ArchiveBackend, BornMember, ENTRY_CHUNK, spill_entry, unpack_failure};
use confit_driver as driver;

/// Streaming gzip member listing and extraction.
///
/// The lone member name derives from the archive file
/// name without its gzip suffix.
#[derive(Debug, Clone, Copy)]
pub(crate) struct GzipBackend;

impl ArchiveBackend for GzipBackend {
    fn names(&self, source: &Path) -> Result<Vec<String>> {
        let file = driver::open_read(source)
            .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
        let mut decoder = flate2::read::GzDecoder::new(file);
        let mut chunk = [0u8; ENTRY_CHUNK];
        loop {
            let read = decoder
                .read(&mut chunk)
                .map_err(|error| unpack_failure(source, error))?;
            if read == 0 {
                break;
            }
        }
        Ok(vec![member_name(source)])
    }

    fn unpack(&self, source: &Path, staging: &Path) -> Result<Vec<BornMember>> {
        let file = driver::open_read(source)
            .map_err(|error| Error::Plan(format!("cannot read '{}': {error}", source.display())))?;
        let name = member_name(source);
        Ok(vec![spill_entry(
            source,
            staging,
            &name,
            flate2::read::GzDecoder::new(file),
        )?])
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
