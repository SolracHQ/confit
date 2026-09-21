//! Archive
//!
//! Byte readers plus extract-once scratch for compressed archives.

use std::path::{Path, PathBuf};

use sha2::Digest as _;

use crate::error::plan_error;
use std::io::Read as _;

/// Scratch folder name under the process temp dir.
const EXTRACT_DIR_NAME: &str = "confit-extract";

/// Chunk size for streaming archive files into the content hash.
const HASH_CHUNK: usize = 8 * 1024;

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

/// Fixed scratch root holding one folder per archive hash.
pub(crate) fn extract_root() -> PathBuf {
    std::env::temp_dir().join(EXTRACT_DIR_NAME)
}

/// Streams one archive file into its content hash.
///
/// # Errors
///
/// Missing plus unreadable files fail as plan errors.
pub(crate) fn archive_sha(full: &Path, rel: &str, ctor: &str) -> mlua::Result<String> {
    let mut file = std::fs::File::open(full)
        .map_err(|error| plan_error(format!("{ctor}: cannot read '{rel}': {error}")))?;
    let mut hash = sha2::Sha256::new();
    let mut chunk = [0u8; HASH_CHUNK];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|error| plan_error(format!("{ctor}: cannot read '{rel}': {error}")))?;
        if read == 0 {
            break;
        }
        hash.update(&chunk[..read]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Unpacks one archive once into its hash folder.
///
/// A present folder returns at once, so near runs reuse it.
/// A fresh unpack lands in a `.part` folder renamed on
/// completion, so a dead process leaves no half folder.
///
/// # Errors
///
/// Unreadable archives fail as plan errors. Members escaping
/// the folder fail as plan errors. Write failures fail as
/// plan errors.
pub(crate) fn ensure_extracted(
    full: &Path,
    rel: &str,
    ctor: &str,
    sha: &str,
) -> mlua::Result<PathBuf> {
    let root = extract_root();
    let dest = root.join(sha);
    if dest.is_dir() {
        return Ok(dest);
    }
    std::fs::create_dir_all(&root)
        .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
    let part = root.join(format!("{sha}.part"));
    if part.exists() {
        std::fs::remove_dir_all(&part)
            .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
    }
    std::fs::create_dir_all(&part)
        .map_err(|error| plan_error(format!("{ctor}: cannot unpack '{rel}': {error}")))?;
    let result: mlua::Result<Vec<RawMember>> = (|| {
        let bytes = std::fs::read(full)
            .map_err(|error| plan_error(format!("{ctor}: cannot read '{rel}': {error}")))?;
        read_members(&bytes, rel, ctor)
    })();
    let members = match result {
        Ok(members) => members,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&part);
            return Err(error);
        }
    };
    for member in &members {
        check_member_path(&member.name, rel, ctor)?;
        let path = part.join(&member.name);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            let _ = std::fs::remove_dir_all(&part);
            return Err(plan_error(format!(
                "{ctor}: cannot unpack '{rel}': {error}"
            )));
        }
        if let Err(error) = std::fs::write(&path, &member.content) {
            let _ = std::fs::remove_dir_all(&part);
            return Err(plan_error(format!(
                "{ctor}: cannot unpack '{rel}': {error}"
            )));
        }
    }
    match std::fs::rename(&part, &dest) {
        Ok(()) => Ok(dest),
        Err(_) if dest.is_dir() => {
            let _ = std::fs::remove_dir_all(&part);
            Ok(dest)
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&part);
            Err(plan_error(format!(
                "{ctor}: cannot unpack '{rel}': {error}"
            )))
        }
    }
}

/// Rejects member paths escaping the extract folder.
///
/// # Errors
///
/// Empty plus absolute plus dot-dot members fail as plan errors.
fn check_member_path(name: &str, rel: &str, ctor: &str) -> mlua::Result<()> {
    if name.is_empty() {
        return Err(plan_error(format!(
            "{ctor}: cannot unpack '{rel}': empty member path"
        )));
    }
    if Path::new(name).is_absolute() {
        return Err(plan_error(format!(
            "{ctor}: cannot unpack '{rel}': member '{name}' escapes"
        )));
    }
    for segment in name.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(plan_error(format!(
                "{ctor}: cannot unpack '{rel}': member '{name}' escapes"
            )));
        }
    }
    Ok(())
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
