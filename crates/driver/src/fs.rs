//! Reads and writes the host filesystem with confit error context.

use std::io;
use std::path::{Path, PathBuf};

use confit_core::error::{ContextualCause, ContextualResult, FilesystemError};

/// Creates the parent folder of a file path.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the parent folder cannot be created.
/// - [`FilesystemError::FilePermissionDenied`] when the parent folder is not writable.
pub fn create_parent(path: &Path) -> ContextualResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| io_context(error, path))?;
    }
    Ok(())
}

/// Returns the immediate children of a folder sorted by name.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the folder is missing.
/// - [`FilesystemError::FilePermissionDenied`] when the folder is unreadable.
/// - [`FilesystemError::FileUnknown`] when the path is a file.
pub fn read_dir(path: &Path) -> ContextualResult<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|error| io_context(error, path))? {
        entries.push(entry.map_err(|error| io_context(error, path))?.path());
    }
    entries.sort();
    Ok(entries)
}

/// Writes a symlink and replaces any entry at the link path.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the parent folder cannot be created.
/// - [`FilesystemError::FilePermissionDenied`] when the folder is not writable.
pub fn write_link(link: &Path, target: &Path) -> ContextualResult<()> {
    create_parent(link)?;
    if std::fs::symlink_metadata(link).is_ok() {
        std::fs::remove_file(link).map_err(|error| io_context(error, link))?;
    }
    std::os::unix::fs::symlink(target, link).map_err(|error| io_context(error, link))
}

/// Removes a symlink and keeps the other entries.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the link is missing.
/// - [`FilesystemError::FilePermissionDenied`] when the folder is not writable.
pub fn remove_link(path: &Path) -> ContextualResult<()> {
    if path.is_symlink() {
        std::fs::remove_file(path).map_err(|error| io_context(error, path))?;
    }
    Ok(())
}

/// Reads the target of the symlink as bytes.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the path is missing.
/// - [`FilesystemError::FilePermissionDenied`] when the path is unreadable.
pub fn read_link_bytes(path: &Path) -> ContextualResult<Vec<u8>> {
    Ok(std::fs::read_link(path)
        .map_err(|error| io_context(error, path))?
        .as_os_str()
        .as_encoded_bytes()
        .to_vec())
}

/// The permission bits used when a path carries no mode.
const DEFAULT_MODE: u32 = 0o644;

/// Writes the unix permission bits onto a path.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the path is missing.
/// - [`FilesystemError::FilePermissionDenied`] when the path is not writable.
pub fn set_mode(path: &Path, mode: Option<u32>) -> ContextualResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let bits = match mode {
        Some(bits) => bits,
        None => DEFAULT_MODE,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))
        .map_err(|error| io_context(error, path))
}

/// Returns the unix permission bits that the path carries.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] when the path is missing.
/// - [`FilesystemError::FilePermissionDenied`] when the path is unreadable.
pub fn mode(path: &Path) -> ContextualResult<u32> {
    use std::os::unix::fs::PermissionsExt;

    let facts = std::fs::symlink_metadata(path).map_err(|error| io_context(error, path))?;
    Ok(facts.permissions().mode() & 0o777)
}

/// Attaches the attempted path to an io failure.
pub(crate) fn io_context(error: io::Error, path: &Path) -> ContextualCause {
    ContextualCause::from(FilesystemError::from(error)).with_fs_path(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_link_keeps_the_plain_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.txt");
        std::fs::write(&target, b"linked bytes").unwrap();
        remove_link(&target).unwrap();
        assert!(
            target.is_file(),
            "the plain file is present after link removal"
        );
    }

    #[test]
    fn host_links_remove_and_keep_their_targets() {
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
        assert!(
            target.is_file(),
            "the plain file is present after link removal"
        );
    }
}
