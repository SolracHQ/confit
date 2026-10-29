//! Filesystem helpers that add semantics over `std::fs`.

use std::io;
use std::path::{Path, PathBuf};

use confit_core::error::{ContextualCause, ContextualResult, FilesystemError};

/// Build the parent folder for one file path.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing parents.
/// - [`FilesystemError::FilePermissionDenied`] for unwritable parents.
pub fn create_parent(path: &Path) -> ContextualResult<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(|error| io_context(error, path))?;
    }
    Ok(())
}

/// List immediate child paths under one folder sorted by name.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing folders.
/// - [`FilesystemError::FilePermissionDenied`] for unreadable folders.
/// - [`FilesystemError::FileUnknown`] for file paths passed as folders.
pub fn read_dir(path: &Path) -> ContextualResult<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path).map_err(|error| io_context(error, path))? {
        entries.push(entry.map_err(|error| io_context(error, path))?.path());
    }
    entries.sort();
    Ok(entries)
}

/// Create one symlink and replace any present entry.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing parents.
/// - [`FilesystemError::FilePermissionDenied`] for unwritable folders.
pub fn write_link(link: &Path, target: &Path) -> ContextualResult<()> {
    create_parent(link)?;
    if std::fs::symlink_metadata(link).is_ok() {
        std::fs::remove_file(link).map_err(|error| io_context(error, link))?;
    }
    std::os::unix::fs::symlink(target, link).map_err(|error| io_context(error, link))
}

/// Remove one symlink without touching other entries.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing links.
/// - [`FilesystemError::FilePermissionDenied`] for unwritable folders.
pub fn remove_link(path: &Path) -> ContextualResult<()> {
    if path.is_symlink() {
        std::fs::remove_file(path).map_err(|error| io_context(error, path))?;
    }
    Ok(())
}

/// Read one symlink target as bytes.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing paths.
/// - [`FilesystemError::FilePermissionDenied`] for unreadable paths.
pub fn read_link_bytes(path: &Path) -> ContextualResult<Vec<u8>> {
    Ok(std::fs::read_link(path)
        .map_err(|error| io_context(error, path))?
        .as_os_str()
        .as_encoded_bytes()
        .to_vec())
}

/// Default permission bits for one path.
const DEFAULT_MODE: u32 = 0o644;

/// Set unix permission bits on one path.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing paths.
/// - [`FilesystemError::FilePermissionDenied`] for unwritable paths.
pub fn set_mode(path: &Path, mode: Option<u32>) -> ContextualResult<()> {
    use std::os::unix::fs::PermissionsExt;

    let bits = match mode {
        Some(bits) => bits,
        None => DEFAULT_MODE,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(bits))
        .map_err(|error| io_context(error, path))
}

/// Read unix permission bits for one path.
///
/// # Errors
///
/// - [`FilesystemError::FileMissing`] for missing paths.
/// - [`FilesystemError::FilePermissionDenied`] for unreadable paths.
pub fn mode(path: &Path) -> ContextualResult<u32> {
    use std::os::unix::fs::PermissionsExt;

    let facts = std::fs::symlink_metadata(path).map_err(|error| io_context(error, path))?;
    Ok(facts.permissions().mode() & 0o777)
}

/// Map one io failure to its context with the attempted path.
pub(crate) fn io_context(error: io::Error, path: &Path) -> ContextualCause {
    ContextualCause::from(FilesystemError::from(error)).with_fs_path(path)
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
