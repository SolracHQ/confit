//! Zip framing.

use std::io;

pub use zip::result::ZipError;

/// Zip local file header magic.
const LOCAL_MAGIC: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];

/// Zip empty archive magic.
const EMPTY_MAGIC: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];

/// One zip read member.
pub struct Member<'a> {
    /// Container name with forward slashes.
    pub name: String,
    /// Reader for the member body bytes.
    pub reader: Box<dyn io::Read + 'a>,
}

/// Report whether zip bytes fill one head window.
///
/// Local file magic and empty archive magic return true, short and plain
/// heads return false.
pub fn valid(head: &[u8]) -> bool {
    head.starts_with(&LOCAL_MAGIC) || head.starts_with(&EMPTY_MAGIC)
}

/// List file member names from a zip stream.
///
/// - Folders with non-files and empty names skip.
///
/// # Errors
///
/// - Unreadable streams fail as zip errors.
/// - [InvalidPassword] when entries are locked.
/// - [UnsupportedArchive] when features are unknown.
/// - [CompressionMethodNotSupported] when methods refuse.
/// - [InvalidArchive] with [FileNotFound] when bytes corrupt.
pub fn list_names(reader: impl io::Read + io::Seek) -> Result<Vec<String>, ZipError> {
    let mut archive = zip::ZipArchive::new(reader)?;
    let mut names = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
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

/// Walk file members from a zip stream.
///
/// - Folders with non-files and empty names skip.
///
/// # Errors
///
/// - Stream, entry, and visitor failures fail as zip errors.
/// - [InvalidPassword] when entries are locked.
/// - [UnsupportedArchive] when features are unknown.
/// - [CompressionMethodNotSupported] when methods refuse.
/// - [InvalidArchive] with [FileNotFound] when bytes corrupt.
pub fn walk(
    reader: impl io::Read + io::Seek,
    mut visit: impl FnMut(Member<'_>) -> io::Result<()>,
) -> Result<(), ZipError> {
    let mut archive = zip::ZipArchive::new(reader)?;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().to_owned();
        if name.is_empty() {
            continue;
        }
        let member = Member {
            name,
            reader: Box::new(entry),
        };
        visit(member).map_err(ZipError::Io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::io::Write as _;

    use super::*;

    fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, bytes) in members {
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn walk_bodies(reader: impl io::Read + io::Seek) -> Vec<(String, Vec<u8>)> {
        let mut found = Vec::new();
        walk(reader, |member| {
            let Member { name, mut reader } = member;
            let mut body = Vec::new();
            reader.read_to_end(&mut body).unwrap();
            found.push((name, body));
            Ok(())
        })
        .unwrap();
        found
    }

    #[test]
    fn valid_returns_local_plus_empty_magic() {
        assert!(valid(&zip_bytes(&[("a.txt", b"alpha")])));
        assert!(valid(&zip_bytes(&[])));
        assert!(!valid(b"plain text"));
        assert!(!valid(&[]));
        assert!(!valid(&[0x50, 0x4b]));
    }

    #[test]
    fn list_plus_walk_read_named_members() {
        let raw = zip_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        assert_eq!(
            list_names(io::Cursor::new(raw.as_slice())).unwrap(),
            vec!["a.txt", "sub/b.txt"]
        );
        assert_eq!(
            walk_bodies(io::Cursor::new(raw.as_slice())),
            vec![
                ("a.txt".to_owned(), b"alpha".to_vec()),
                ("sub/b.txt".to_owned(), b"beta".to_vec()),
            ]
        );
    }

    #[test]
    fn corrupt_zip_fails_as_invalid_archive() {
        let outcome = list_names(io::Cursor::new(b"plain text"));
        assert!(
            matches!(outcome.unwrap_err(), ZipError::InvalidArchive(_)),
            "plain bytes refuse as invalid archive"
        );
    }
}
