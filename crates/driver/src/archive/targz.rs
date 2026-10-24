//! Tar member reading.
//!
//! Readers visit plain and gzipped tar members.

use std::io;
use std::io::Read as _;

use super::Error;
use super::Result;

/// Gzip magic for deflated streams.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Offset of the tar format tag.
const TAR_MAGIC_OFFSET: usize = 257;

/// Tar magic tag for the format.
const TAR_MAGIC: &[u8] = b"ustar";

/// One tar block for the decode window.
const TAR_BLOCK: u64 = 512;

/// Fallback mode for missing permission bits.
const FALLBACK_MODE: u32 = 0o644;

/// Permission mask for exposed modes.
const MODE_MASK: u32 = 0o777;

/// One tar file head with its name and mode.
struct NamedFile {
    /// Container name with forward slashes.
    name: String,
    /// Unix permission bits.
    mode: u32,
}

/// One sniffed stream with its gzip verdict.
enum Probed<R> {
    /// Gzipped stream with bytes chained back.
    Gzipped(io::Chain<io::Cursor<Vec<u8>>, R>),
    /// Plain stream with bytes chained back.
    Plain(io::Chain<io::Cursor<Vec<u8>>, R>),
}

/// Return tar member names in archive order.
///
/// - Every returned name is a file with a non-empty name.
///
/// # Errors
///
/// - [`Error::Bare`] for bare gzip singles.
/// - [`Error::Corrupt`] for corrupt decodes.
/// - [`Error::Truncated`] for cut streams.
/// - [`Error::Denied`] for denied access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
pub(super) fn names(reader: impl io::Read) -> Result<Vec<String>> {
    match sniff(reader)? {
        Probed::Plain(stream) => plain_names(stream),
        Probed::Gzipped(stream) => {
            let windowed = gunzip_window(stream)?;
            plain_names(windowed)
        }
    }
}

/// Walk tar members with one visitor.
///
/// - Every visited member is a file with a non-empty name.
/// - Modes mask toward 0o777 with 0o644 fallback.
///
/// # Errors
///
/// - [`Error::Bare`] for bare gzip singles.
/// - [`Error::Corrupt`] for corrupt decodes.
/// - [`Error::Truncated`] for cut streams.
/// - [`Error::Denied`] for denied access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
/// - [`Error::Visit`] for failing visitors.
pub(super) fn walk<E>(
    reader: impl io::Read,
    visit: impl FnMut(super::Entry<'_>) -> std::result::Result<(), E>,
) -> Result<(), E> {
    match sniff(reader)? {
        Probed::Plain(stream) => plain_walk(stream, visit),
        Probed::Gzipped(stream) => {
            let windowed = gunzip_window(stream)?;
            plain_walk(windowed, visit)
        }
    }
}

/// Sniff gzip magic off one stream.
///
/// # Errors
///
/// - [`Error::Denied`] for denied access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams.
fn sniff<R: io::Read, E>(mut reader: R) -> Result<Probed<R>, E> {
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

/// Decode one tar block window.
///
/// Empty windows read as tarred.
///
/// # Errors
///
/// - [`Error::Bare`] for bare singles.
/// - [`Error::Corrupt`] for corrupt decodes.
/// - [`Error::Truncated`] for cut streams.
/// - [`Error::Denied`] for denied access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams.
fn gunzip_window<R: io::Read, E>(
    reader: R,
) -> Result<io::Chain<io::Cursor<Vec<u8>>, flate2::read::GzDecoder<R>>, E> {
    let mut decoder = flate2::read::GzDecoder::new(reader);
    let mut window = Vec::with_capacity(TAR_BLOCK as usize);
    decoder.by_ref().take(TAR_BLOCK).read_to_end(&mut window)?;
    if window.is_empty() || is_tar_tag(&window) {
        Ok(io::Cursor::new(window).chain(decoder))
    } else {
        Err(Error::Bare)
    }
}

/// Report whether tar bytes fill one head window.
fn is_tar_tag(head: &[u8]) -> bool {
    head.get(TAR_MAGIC_OFFSET..TAR_MAGIC_OFFSET + TAR_MAGIC.len())
        .is_some_and(|tag| tag == TAR_MAGIC)
}

/// Return file names from one plain tar stream.
///
/// - Every returned name is a file with a non-empty name.
///
/// # Errors
///
/// - [`Error::Denied`] for denied access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
fn plain_names(reader: impl io::Read) -> Result<Vec<String>> {
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

/// Walk file members from one plain tar stream.
///
/// - Every visited member is a file with a non-empty name.
///
/// # Errors
///
/// - [`Error::Denied`] for denied access.
/// - [`Error::Missing`] for missing input.
/// - [`Error::Io`] for failed streams and entries.
/// - [`Error::Visit`] for failing visitors.
fn plain_walk<E>(
    reader: impl io::Read,
    mut visit: impl FnMut(super::Entry<'_>) -> std::result::Result<(), E>,
) -> Result<(), E> {
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
        visit(member).map_err(Error::Visit)?;
    }
    Ok(())
}

/// Collect one file head from a tar entry.
///
/// # Errors
///
/// - [`Error::Io`] for failed paths.
fn file_head<R: io::Read, E>(entry: &mut tar::Entry<'_, R>) -> Result<Option<NamedFile>, E> {
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
