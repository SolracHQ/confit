//! Reads tar members.

use std::io;
use std::io::Read as _;

use confit_core::error::{ArchiveError, ContextualCause, ContextualResult};

/// The magic bytes that open a gzip stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// The byte offset of the tar format tag.
const TAR_MAGIC_OFFSET: usize = 257;

/// The tag that marks the tar format.
const TAR_MAGIC: &[u8] = b"ustar";

/// The tar block size in bytes.
const TAR_BLOCK: u64 = 512;

/// The permission bits used when an entry holds no mode.
const FALLBACK_MODE: u32 = 0o644;

/// The mask that keeps the exposed permission bits.
const MODE_MASK: u32 = 0o777;

/// A file entry inside a tar archive.
struct NamedFile {
    /// The name of the file with forward slashes.
    name: String,
    /// The permission bits of the file.
    mode: u32,
}

/// The stream after a gzip check.
enum Probed<R> {
    /// A gzip stream that starts with the checked prefix.
    Gzipped(io::Chain<io::Cursor<Vec<u8>>, R>),
    /// A plain stream that starts with the checked prefix.
    Plain(io::Chain<io::Cursor<Vec<u8>>, R>),
}

/// Collects the tar member names in archive order.
///
/// # Errors
///
/// - [`ArchiveError::BareGzip`] when a gzip stream holds no tar.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
pub(super) fn names(reader: impl io::Read) -> ContextualResult<Vec<String>> {
    match sniff(reader)? {
        Probed::Plain(stream) => plain_names(stream),
        Probed::Gzipped(stream) => {
            let windowed = gunzip_window(stream)?;
            plain_names(windowed)
        }
    }
}

/// Calls the visitor for each tar member.
///
/// # Errors
///
/// - [`ArchiveError::BareGzip`] when a gzip stream holds no tar.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
pub(super) fn walk(
    reader: impl io::Read,
    visit: impl FnMut(super::Entry<'_>) -> ContextualResult<()>,
) -> ContextualResult<()> {
    match sniff(reader)? {
        Probed::Plain(stream) => plain_walk(stream, visit),
        Probed::Gzipped(stream) => {
            let windowed = gunzip_window(stream)?;
            plain_walk(windowed, visit)
        }
    }
}

/// Checks the stream head for the gzip magic.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
fn sniff<R: io::Read>(mut reader: R) -> ContextualResult<Probed<R>> {
    let mut prefix = Vec::with_capacity(GZIP_MAGIC.len());
    let mut byte = [0u8; 1];
    while prefix.len() < GZIP_MAGIC.len() {
        let read = io::Read::read(&mut reader, &mut byte)?;
        if read == 0 {
            break;
        }
        prefix.push(byte[0]);
    }
    let gzipped = prefix.as_slice() == GZIP_MAGIC;
    let stream = io::Cursor::new(prefix).chain(reader);
    if gzipped {
        Ok(Probed::Gzipped(stream))
    } else {
        Ok(Probed::Plain(stream))
    }
}

/// Decodes the first tar block from the gzip stream.
///
/// # Errors
///
/// - [`ArchiveError::BareGzip`] when a gzip stream holds no tar.
/// - [`confit_core::error::FilesystemError`] when access is denied or the input is missing.
fn gunzip_window<R: io::Read>(
    reader: R,
) -> ContextualResult<io::Chain<io::Cursor<Vec<u8>>, flate2::read::GzDecoder<R>>> {
    let mut decoder = flate2::read::GzDecoder::new(reader);
    let mut window = Vec::with_capacity(TAR_BLOCK as usize);
    decoder.by_ref().take(TAR_BLOCK).read_to_end(&mut window)?;
    if window.is_empty() || is_tar_tag(&window) {
        Ok(io::Cursor::new(window).chain(decoder))
    } else {
        Err(ContextualCause::from(ArchiveError::BareGzip))
    }
}

/// Returns whether the head bytes hold a tar tag.
fn is_tar_tag(head: &[u8]) -> bool {
    head.get(TAR_MAGIC_OFFSET..TAR_MAGIC_OFFSET + TAR_MAGIC.len())
        .is_some_and(|tag| tag == TAR_MAGIC)
}

/// Collects the file names from a plain tar stream.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] when access is denied, the input is missing, or an entry is corrupt.
fn plain_names(reader: impl io::Read) -> ContextualResult<Vec<String>> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries()?;
    let mut found = Vec::new();
    for entry in entries {
        let mut entry = entry?;
        let Some(head) = file_head(&mut entry)? else {
            io::copy(&mut entry, &mut io::sink())?;
            continue;
        };
        io::copy(&mut entry, &mut io::sink())?;
        found.push(head.name);
    }
    Ok(found)
}

/// Calls the visitor for each file member of a plain tar stream.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] when access is denied, the input is missing, or an entry is corrupt.
fn plain_walk(
    reader: impl io::Read,
    mut visit: impl FnMut(super::Entry<'_>) -> ContextualResult<()>,
) -> ContextualResult<()> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries()?;
    for entry in entries {
        let mut entry = entry?;
        let Some(found) = file_head(&mut entry)? else {
            io::copy(&mut entry, &mut io::sink())?;
            continue;
        };
        let member = super::Entry {
            name: found.name,
            mode: found.mode,
            reader: Box::new(entry),
        };
        visit(member)?;
    }
    Ok(())
}

/// Returns the file head when the entry is a file.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] when the entry path fails.
fn file_head<R: io::Read>(entry: &mut tar::Entry<'_, R>) -> ContextualResult<Option<NamedFile>> {
    let kind = entry.header().entry_type();
    if kind.is_dir() || !kind.is_file() {
        return Ok(None);
    }
    let name = entry.path()?.to_string_lossy().into_owned();
    if name.is_empty() {
        return Ok(None);
    }
    let mode = entry
        .header()
        .mode()
        .map(|mode| mode & MODE_MASK)
        .unwrap_or(FALLBACK_MODE);
    Ok(Some(NamedFile { name, mode }))
}
