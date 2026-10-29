//! Atomic file writes through staged sibling temp files.

use std::path::{Path, PathBuf};

use confit_core::error::ContextualResult;

use crate::fs::{create_parent, io_context};

/// Staged scratch file under one destination folder.
pub struct StagedFile {
    /// The staged scratch file.
    file: tempfile::NamedTempFile,
}

impl StagedFile {
    /// Stage one empty scratch file under the destination folder.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] for missing parents.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] for unwritable destinations.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] for scratch staging faults.
    pub fn stage(dest: &Path) -> ContextualResult<Self> {
        create_parent(dest)?;
        tempfile::NamedTempFile::new_in(scratch_dir(dest))
            .map(|file| Self { file })
            .map_err(|error| io_context(error, dest))
    }

    /// Write one file atomically through a sibling rename.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] for missing parents.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] for unwritable destinations.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] for staging and write faults.
    pub fn write(
        dest: &Path,
        write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    ) -> ContextualResult<()> {
        let mut staged = Self::stage(dest)?;
        write(staged.file.as_file_mut()).map_err(|error| io_context(error, dest))?;
        staged.persist(dest)
    }

    /// Return the scratch path for direct writes.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Move the scratch file onto the destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] for unwritable destinations.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] for persist faults with an absent destination.
    pub fn persist(self, dest: &Path) -> ContextualResult<()> {
        match self.file.persist(dest) {
            Ok(_) => Ok(()),
            Err(_) if dest.exists() => Ok(()),
            Err(error) => Err(io_context(error.error, dest)),
        }
    }
}

/// Staged scratch dir under one destination folder.
pub struct StagedDir {
    /// The staged scratch dir.
    dir: tempfile::TempDir,
}

impl StagedDir {
    /// Stage one empty scratch dir under the destination folder.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] for missing parents.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] for unwritable destinations.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] for scratch staging faults.
    pub fn stage(dest: &Path) -> ContextualResult<Self> {
        create_parent(dest)?;
        tempfile::TempDir::new_in(scratch_dir(dest))
            .map(|dir| Self { dir })
            .map_err(|error| io_context(error, dest))
    }

    /// Return the scratch path for direct writes.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Rename the scratch dir onto the destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] for missing scratch entries.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] for unwritable destinations.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] for rename faults with an absent destination.
    pub fn persist(self, dest: &Path) -> ContextualResult<()> {
        match std::fs::rename(self.path(), dest) {
            Ok(()) => {
                let _ = self.dir.keep();
                Ok(())
            }
            Err(_) if dest.exists() => Ok(()),
            Err(error) => Err(io_context(error, dest)),
        }
    }
}

/// Resolve the scratch parent for one destination.
fn scratch_dir(dest: &Path) -> PathBuf {
    dest.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::fs::read_dir;

    #[test]
    fn staged_write_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        StagedFile::write(&dest, |file| {
            use std::io::Write as _;
            file.write_all(b"atomic bytes")
        })
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"atomic bytes");
    }

    #[test]
    fn staged_write_failure_cleans_pending() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        let outcome = StagedFile::write(&dest, |_| {
            Err(std::io::Error::other("scripted write failure"))
        });
        assert!(outcome.is_err(), "closure failure reports loud");
        assert!(!dest.exists(), "failed write leaves no destination");
        let parent = dest.parent().unwrap().to_path_buf();
        let leftovers = read_dir(&parent).unwrap_or_default();
        assert!(leftovers.is_empty(), "failed write leaves no pending entry");
    }

    #[test]
    fn staged_write_overwrites_present_destination() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(&dest, b"winner").unwrap();
        StagedFile::write(&dest, |file| {
            use std::io::Write as _;
            file.write_all(b"newcomer")
        })
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"newcomer");
    }
}
