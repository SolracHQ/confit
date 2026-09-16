//! Archive
//!
//! Byte readers for compressed archives.

use std::path::Path;

use crate::error::plan_error;
use std::io::Read as _;

/// Raw archive member with bytes plus mode.
pub(crate) struct RawMember {
    /// Member path inside the archive.
    pub(crate) name: String,
    /// Member size in bytes.
    pub(crate) size: u64,
    /// Executable bit from the tar mode.
    pub(crate) executable: bool,
    /// Raw member bytes.
    pub(crate) content: Vec<u8>,
}

/// Reads archive members from raw bytes.
pub(crate) fn read_members(bytes: &[u8], rel: &str, ctor: &str) -> mlua::Result<Vec<RawMember>> {
    if is_gzip(bytes) {
        let flat = gunzip(bytes, rel, ctor)?;
        match parse_tar(&flat, rel, ctor) {
            Ok(members) => Ok(members),
            Err(_) => {
                if wants_tar(rel) {
                    return Err(plan_error(format!(
                        "{ctor}: cannot unpack '{rel}': not a tar archive"
                    )));
                }
                let size = flat.len() as u64;
                Ok(vec![RawMember {
                    name: single_name(rel),
                    size,
                    executable: false,
                    content: flat,
                }])
            }
        }
    } else if is_zip(bytes) {
        parse_zip(bytes, rel, ctor)
    } else {
        parse_tar(bytes, rel, ctor)
    }
}

/// Reports true for gzip magic bytes.
fn is_gzip(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

/// Reports true for zip magic bytes.
fn is_zip(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'P' && bytes[1] == b'K' && bytes[2] == 3 && bytes[3] == 4
}

/// Reports true while a name wants tar members.
fn wants_tar(rel: &str) -> bool {
    let lower = rel.to_lowercase();
    lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".tar")
}

/// Derives the single-file member name from an archive path.
fn single_name(rel: &str) -> String {
    let base = Path::new(rel)
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

/// Gunzips one archive body.
fn gunzip(bytes: &[u8], rel: &str, ctor: &str) -> mlua::Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut flat = Vec::new();
    decoder
        .read_to_end(&mut flat)
        .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
    Ok(flat)
}

/// Parses tar members from one byte slice.
fn parse_tar(bytes: &[u8], rel: &str, ctor: &str) -> mlua::Result<Vec<RawMember>> {
    let mut archive = tar::Archive::new(bytes);
    let entries = archive
        .entries()
        .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
    let mut out = Vec::new();
    for entry in entries {
        let mut entry =
            entry.map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
        let kind = entry.header().entry_type();
        if kind.is_dir() || !kind.is_file() {
            continue;
        }
        let name = entry
            .path()
            .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?
            .to_string_lossy()
            .into_owned();
        if name.is_empty() {
            continue;
        }
        let mode = entry
            .header()
            .mode()
            .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
        let size = entry.size();
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
        out.push(RawMember {
            name,
            size,
            executable: mode & 0o111 != 0,
            content,
        });
    }
    Ok(out)
}

/// Parses zip members from one byte slice.
fn parse_zip(bytes: &[u8], rel: &str, ctor: &str) -> mlua::Result<Vec<RawMember>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
    let mut out = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();
        if name.is_empty() {
            continue;
        }
        let size = entry.size();
        let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
        let mut content = Vec::new();
        entry
            .read_to_end(&mut content)
            .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
        out.push(RawMember {
            name,
            size,
            executable,
            content,
        });
    }
    Ok(out)
}
