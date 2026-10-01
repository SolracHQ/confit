//! Tar framing over caller streams with io errors.
//!
//! Names with bodies ride caller streams, so
//! stores own seals with errors.

use std::io;
use std::io::Read as _;

use super::gzip;

/// Gzip magic heading every deflated stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Tar magic offset holding the format tag.
const TAR_MAGIC_OFFSET: usize = 257;

/// Tar magic tag naming the format.
const TAR_MAGIC: &[u8] = b"ustar";

/// One tar block feeding the format probe.
const TAR_BLOCK: u64 = 512;

/// One tar read member over a caller reader.
///
/// Bodies stream without staging, so walk
/// output stays lean.
pub struct Member<'a> {
    /// Container name with forward slashes.
    pub name: String,
    /// Reader yielding the member body bytes.
    pub reader: Box<dyn io::Read + 'a>,
}

/// One tar build member over a caller reader.
///
/// Length with mode feed entry headers, so
/// built containers carry exact framing.
pub struct BuildMember<'a> {
    /// Container name with forward slashes.
    pub name: String,
    /// Byte count recorded in the header.
    pub len: u64,
    /// Reader yielding exactly `len` bytes.
    pub reader: Box<dyn io::Read + 'a>,
    /// Unix permission bits for the entry.
    pub mode: u32,
}

/// Reports whether one head window holds tar or gzip bytes.
///
/// Gzip magic and tar magic answer true, short
/// and plain heads answer false.
pub fn handles(head: &[u8]) -> bool {
    is_gzip(head) || is_tar(head)
}

/// Lists file member names from a tar stream.
///
/// Gzip bytes decode first: tar members list named
/// while one bare member lists under the fallback.
/// Folders with non-files and empty names skip, the
/// fallback stays ignored on named members.
///
/// # Errors
///
/// - Stream reads fail as io errors.
/// - [InvalidData] when gzip bytes refuse.
/// - [UnexpectedEof] when gzip bytes cut mid stream.
/// - [Other] when tar framing fails and when an empty
///   fallback meets a nameless single.
pub fn list_names(reader: impl io::Read, fallback: &str) -> io::Result<Vec<String>> {
    let probed = probe(reader)?;
    if !probed.gzipped {
        return plain_names(probed.stream);
    }
    let windowed = gunzip_window(probed.stream)?;
    if windowed.tarred {
        return plain_names(windowed.stream);
    }
    single_names(fallback)
}

/// Walks file members from a tar stream.
///
/// Gzip bytes decode first: tar members walk named
/// while one bare member walks under the fallback.
/// Folders with non-files and empty names skip. The
/// visitor owns each body reader and reads it to end
/// before return. The fallback stays ignored on named
/// members.
///
/// # Errors
///
/// - Stream, entry, and visitor failures fail as io errors.
/// - [InvalidData] when gzip bytes refuse.
/// - [UnexpectedEof] when gzip bytes cut mid stream.
/// - [Other] when tar framing fails and when an empty
///   fallback meets a nameless single.
pub fn walk(
    reader: impl io::Read,
    fallback: &str,
    visit: impl FnMut(Member<'_>) -> io::Result<()>,
) -> io::Result<()> {
    let probed = probe(reader)?;
    if !probed.gzipped {
        return plain_walk(probed.stream, visit);
    }
    let windowed = gunzip_window(probed.stream)?;
    if windowed.tarred {
        return plain_walk(windowed.stream, visit);
    }
    single_walk(windowed.stream, fallback, visit)
}

/// Builds one plain tar over a caller sink.
///
/// Entries land in member order with exact framing.
///
/// # Errors
///
/// - Sink and entry writes fail as io errors.
pub fn build_plain(sink: impl io::Write, members: Vec<BuildMember<'_>>) -> io::Result<()> {
    let mut builder = tar::Builder::new(sink);
    for member in members {
        append_member(&mut builder, member)?;
    }
    builder.into_inner()?;
    Ok(())
}

/// Builds one gzipped tar over a caller sink.
///
/// Entries land in member order at the given
/// deflate level.
///
/// # Errors
///
/// - Sink and entry writes fail as io errors.
pub fn build_gzipped(
    sink: impl io::Write,
    members: Vec<BuildMember<'_>>,
    level: u32,
) -> io::Result<()> {
    let level = flate2::Compression::new(level);
    let encoder = flate2::write::GzEncoder::new(sink, level);
    let mut builder = tar::Builder::new(encoder);
    for member in members {
        append_member(&mut builder, member)?;
    }
    let encoder = builder.into_inner()?;
    encoder.finish()?;
    Ok(())
}

/// One probed stream with its gzip verdict.
struct Probed<R> {
    gzipped: bool,
    stream: io::Chain<io::Cursor<Vec<u8>>, R>,
}

/// Sniffs gzip magic off one stream chaining bytes back.
///
/// Short reads chain what they hold, so plain
/// framing still sees every byte.
///
/// # Errors
///
/// - Stream reads fail as io errors.
fn probe<R: io::Read>(mut reader: R) -> io::Result<Probed<R>> {
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
    Ok(Probed { gzipped, stream })
}

/// Reports gzip magic for one head window.
fn is_gzip(head: &[u8]) -> bool {
    head.starts_with(&GZIP_MAGIC)
}

/// Reports tar magic for one head window.
fn is_tar(head: &[u8]) -> bool {
    head.get(TAR_MAGIC_OFFSET..TAR_MAGIC_OFFSET + TAR_MAGIC.len())
        .is_some_and(|tag| tag == TAR_MAGIC)
}

/// One gunzipped window with its decoder remainder.
///
/// Empty windows read tarred so empty streams walk
/// zero members, matching plain behavior.
struct Windowed<R: io::Read> {
    tarred: bool,
    stream: io::Chain<io::Cursor<Vec<u8>>, gzip::Decoder<R>>,
}

/// Decodes one tar block window chaining bytes back.
///
/// The window decides tar against single without
/// holding the stream. Short reads end the window,
/// so truncated decodes route on what they hold.
///
/// # Errors
///
/// - Stream reads fail as io errors.
/// - [InvalidData] when gzip bytes refuse.
/// - [UnexpectedEof] when gzip bytes cut mid stream.
fn gunzip_window<R: io::Read>(reader: R) -> io::Result<Windowed<R>> {
    let mut decoder = gzip::Decoder::open(reader);
    let mut window = Vec::with_capacity(TAR_BLOCK as usize);
    decoder.by_ref().take(TAR_BLOCK).read_to_end(&mut window)?;
    let tarred = window.is_empty() || is_tar(&window);
    let stream = io::Cursor::new(window).chain(decoder);
    Ok(Windowed { tarred, stream })
}

/// Lists one nameless single under its fallback.
///
/// # Errors
///
/// - [Other] when an empty fallback meets the single.
fn single_names(fallback: &str) -> io::Result<Vec<String>> {
    if fallback.is_empty() {
        return Err(io::Error::other("nameless single refuses empty fallback"));
    }
    Ok(vec![fallback.to_owned()])
}

/// Walks one nameless single under its fallback.
///
/// The visitor owns the body reader and reads
/// it to end before return.
///
/// # Errors
///
/// - Visitor failures fail as io errors.
/// - [Other] when an empty fallback meets the single.
fn single_walk<'a, R: io::Read + 'a>(
    reader: R,
    fallback: &str,
    mut visit: impl FnMut(Member<'a>) -> io::Result<()>,
) -> io::Result<()> {
    if fallback.is_empty() {
        return Err(io::Error::other("nameless single refuses empty fallback"));
    }
    let member = Member {
        name: fallback.to_owned(),
        reader: Box::new(reader),
    };
    visit(member)
}

/// Lists file member names from a plain tar stream.
///
/// Folders with non-files and empty names
/// skip, skipped bytes drain to a sink.
///
/// # Errors
///
/// - Stream and entry reads fail as io errors.
/// - [Other] when tar framing fails.
fn plain_names(reader: impl io::Read) -> io::Result<Vec<String>> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries()?;
    let mut names = Vec::new();
    for entry in entries {
        let mut entry = entry?;
        if let Some(name) = file_name(&mut entry)? {
            names.push(name);
        }
        io::copy(&mut entry, &mut io::sink())?;
    }
    Ok(names)
}

/// Walks file members from a plain tar stream.
///
/// Folders with non-files and empty names
/// skip. The visitor owns each body reader
/// and reads it to end before return.
///
/// # Errors
///
/// - Stream, entry, and visitor failures fail as io errors.
/// - [Other] when tar framing fails.
fn plain_walk(
    reader: impl io::Read,
    mut visit: impl FnMut(Member<'_>) -> io::Result<()>,
) -> io::Result<()> {
    let mut archive = tar::Archive::new(reader);
    let entries = archive.entries()?;
    for entry in entries {
        let mut entry = entry?;
        let Some(name) = file_name(&mut entry)? else {
            io::copy(&mut entry, &mut io::sink())?;
            continue;
        };
        let member = Member {
            name,
            reader: Box::new(entry),
        };
        visit(member)?;
    }
    Ok(())
}

/// Reads one file name for a tar entry.
///
/// Folders with non-files and empty names
/// skip as absent.
///
/// # Errors
///
/// - Undecodable paths fail as io errors.
fn file_name<R: io::Read>(entry: &mut tar::Entry<'_, R>) -> io::Result<Option<String>> {
    let kind = entry.header().entry_type();
    if kind.is_dir() || !kind.is_file() {
        return Ok(None);
    }
    let name = entry.path()?.to_string_lossy().into_owned();
    if name.is_empty() {
        Ok(None)
    } else {
        Ok(Some(name))
    }
}

/// Appends one member under its own header.
///
/// # Errors
///
/// - Header and append failures fail as io errors.
fn append_member<'a, W: io::Write>(
    builder: &mut tar::Builder<W>,
    member: BuildMember<'a>,
) -> io::Result<()> {
    let BuildMember {
        name,
        len,
        mut reader,
        mode,
    } = member;
    let mut header = tar::Header::new_gnu();
    header.set_size(len);
    header.set_mode(mode);
    header.set_cksum();
    builder.append_data(&mut header, name, &mut reader)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn plain_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, bytes) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, *name, *bytes).unwrap();
        }
        builder.into_inner().unwrap()
    }

    fn gzip_bytes(raw: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(raw).unwrap();
        encoder.finish().unwrap()
    }

    fn walk_names(reader: impl io::Read, fallback: &str) -> Vec<String> {
        let mut names = Vec::new();
        walk(reader, fallback, |member| {
            let Member { name, mut reader } = member;
            let mut body = Vec::new();
            reader.read_to_end(&mut body).unwrap();
            names.push(format!("{name}:{}", body.len()));
            Ok(())
        })
        .unwrap();
        names
    }

    #[test]
    fn handles_answers_gzip_plus_tar_magic() {
        assert!(handles(&gzip_bytes(b"x")[..4]));
        assert!(handles(&plain_bytes(&[("a.txt", b"alpha")])));
        assert!(!handles(b"plain text"));
        assert!(!handles(&[]));
        assert!(!handles(&[0x1f]));
    }

    #[test]
    fn list_ignores_fallback_on_named_members() {
        let plain = plain_bytes(&[("a.txt", b"alpha")]);
        assert_eq!(
            list_names(plain.as_slice(), "ignored").unwrap(),
            vec!["a.txt"]
        );
        let gzipped = gzip_bytes(&plain);
        assert_eq!(
            list_names(gzipped.as_slice(), "ignored").unwrap(),
            vec!["a.txt"]
        );
    }

    #[test]
    fn walk_routes_bare_gzip_to_single_member() {
        let gzipped = gzip_bytes(b"plain");
        assert_eq!(walk_names(gzipped.as_slice(), "note"), vec!["note:5"]);
        assert_eq!(
            list_names(gzipped.as_slice(), "note").unwrap(),
            vec!["note"]
        );
    }

    #[test]
    fn bare_single_refuses_empty_fallback() {
        let gzipped = gzip_bytes(b"plain");
        let outcome = list_names(gzipped.as_slice(), "");
        assert_eq!(outcome.unwrap_err().kind(), io::ErrorKind::Other);
        let mut called = false;
        let outcome = walk(gzipped.as_slice(), "", |_| {
            called = true;
            Ok(())
        });
        assert_eq!(outcome.unwrap_err().kind(), io::ErrorKind::Other);
        assert!(!called);
    }

    #[test]
    fn corrupt_gzip_fails_with_decode_kind() {
        let mut gzipped = gzip_bytes(b"plain");
        gzipped.truncate(gzipped.len() - 3);
        let outcome = list_names(gzipped.as_slice(), "note");
        assert!(outcome.is_err());
        let kind = outcome.unwrap_err().kind();
        assert!(
            matches!(
                kind,
                io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof
            ),
            "decode fails loud: {kind:?}"
        );
    }
}
