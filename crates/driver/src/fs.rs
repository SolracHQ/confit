//! Filesystem helpers that add semantics over `std::fs`.

use std::io;
use std::path::{Path, PathBuf};

/// Build the parent folder for one file path.
///
/// - Empty and parentless paths skip.
///
/// # Errors
///
/// - Parent build failures fail as io errors.
pub fn create_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// List immediate child paths under one folder sorted by name.
///
/// # Errors
///
/// - [NotFound] for missing folders.
/// - [PermissionDenied] for unreadable folders.
/// - [NotADirectory] when the path is a file.
pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        entries.push(entry?.path());
    }
    entries.sort();
    Ok(entries)
}

/// Read the last bytes of one file from the tail.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [PermissionDenied] for unreadable paths.
/// - [UnexpectedEof] when the file runs short of the tail.
pub fn read_tail(path: &Path, tail: u64) -> io::Result<Vec<u8>> {
    use std::io::{Read as _, Seek as _};

    let facts = std::fs::metadata(path)?;
    if facts.len() < tail {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("short file '{}'", path.display()),
        ));
    }
    let mut file = std::fs::File::open(path)?;
    file.seek(io::SeekFrom::End(-(tail as i64)))?;
    let mut out = vec![0u8; tail as usize];
    file.read_exact(&mut out)?;
    Ok(out)
}

/// Create one symlink and replace any present entry.
///
/// # Errors
///
/// - [NotFound] for missing parents.
/// - [PermissionDenied] for unwritable folders.
/// - [InvalidInput] when link paths carry bad bytes.
pub fn write_link(link: &Path, target: &Path) -> io::Result<()> {
    create_parent(link)?;
    if std::fs::symlink_metadata(link).is_ok() {
        std::fs::remove_file(link)?;
    }
    std::os::unix::fs::symlink(target, link)
}

/// Remove one symlink without touching other entries.
///
/// - Links remove, other entries stay.
///
/// # Errors
///
/// - Link check and removal failures fail as io errors.
pub fn remove_link(path: &Path) -> io::Result<()> {
    if path.is_symlink() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Read one symlink target as bytes.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [PermissionDenied] for unreadable paths.
/// - [InvalidInput] for non-link paths.
pub fn read_link_bytes(path: &Path) -> io::Result<Vec<u8>> {
    Ok(std::fs::read_link(path)?
        .as_os_str()
        .as_encoded_bytes()
        .to_vec())
}

/// Default permission bits for one path.
const DEFAULT_MODE: u32 = 0o644;

/// Set unix permission bits on one path.
///
/// - Absent modes take the default.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [PermissionDenied] for unwritable paths.
pub fn set_mode(path: &Path, mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let bits = match mode {
        Some(bits) => bits,
        None => DEFAULT_MODE,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))
}

/// Read unix permission bits for one path.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [PermissionDenied] for unreadable paths.
pub fn mode(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    let facts = std::fs::symlink_metadata(path)?;
    Ok(facts.permissions().mode() & 0o777)
}

/// Read mtime nanos since the epoch, zero when missing or pre-epoch.
pub fn modified_nanos(facts: &std::fs::Metadata) -> u64 {
    facts
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|span| u64::try_from(span.as_nanos()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_link_keeps_plain_files() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"linked bytes").unwrap();
        remove_link(&target).unwrap();
        assert!(target.is_file(), "plain files stay past link removal");
    }

    #[test]
    fn host_links_remove_without_touching_targets() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"linked bytes").unwrap();
        let link = dir.path().join("note");
        write_link(&link, Path::new("target.txt")).unwrap();
        assert!(link.is_symlink());
        assert_eq!(std::fs::read(&link).unwrap(), b"linked bytes".to_vec());
        remove_link(&link).unwrap();
        assert!(!link.exists());
        assert_eq!(std::fs::read(&target).unwrap(), b"linked bytes".to_vec());
        remove_link(&target).unwrap();
        assert!(target.is_file(), "plain files stay past link removal");
    }
}
