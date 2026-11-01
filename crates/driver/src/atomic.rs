//! Writes files atomically through a staged sibling temp file.

use std::path::{Path, PathBuf};

use confit_core::error::ContextualResult;

use crate::fs::{create_parent, io_context};

/// A scratch file staged beside the destination file.
pub struct StagedFile {
    /// The staged scratch file.
    file: tempfile::NamedTempFile,
}

impl StagedFile {
    /// Creates an empty scratch file beside the destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] when the parent folder is missing.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] when the destination is not writable.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] when the scratch file cannot be staged.
    pub fn stage(dest: &Path) -> ContextualResult<Self> {
        create_parent(dest)?;
        tempfile::NamedTempFile::new_in(scratch_dir(dest))
            .map(|file| Self { file })
            .map_err(|error| io_context(error, dest))
    }

    /// Writes a file through a scratch sibling and a rename.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] when the parent folder is missing.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] when the destination is not writable.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] when the staging or the write fails.
    pub fn write(
        dest: &Path,
        write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    ) -> ContextualResult<()> {
        let mut staged = Self::stage(dest)?;
        write(staged.file.as_file_mut()).map_err(|error| io_context(error, dest))?;
        staged.persist(dest)
    }

    /// Returns the path of the scratch file.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Replaces the destination with the scratch file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] when the destination is not writable.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] when the destination is absent and the move fails.
    pub fn persist(self, dest: &Path) -> ContextualResult<()> {
        match self.file.persist(dest) {
            Ok(_) => Ok(()),
            Err(_) if dest.exists() => Ok(()),
            Err(error) => Err(io_context(error.error, dest)),
        }
    }
}

/// A scratch folder staged beside the destination folder.
pub struct StagedDir {
    /// The staged scratch folder.
    dir: tempfile::TempDir,
}

impl StagedDir {
    /// Creates an empty scratch folder beside the destination.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] when the parent folder is missing.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] when the destination is not writable.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] when the scratch folder cannot be staged.
    pub fn stage(dest: &Path) -> ContextualResult<Self> {
        create_parent(dest)?;
        tempfile::TempDir::new_in(scratch_dir(dest))
            .map(|dir| Self { dir })
            .map_err(|error| io_context(error, dest))
    }

    /// Returns the path of the scratch folder.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Replaces the destination with the scratch folder.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError::FileMissing`] when a scratch entry is missing.
    /// - [`confit_core::error::FilesystemError::FilePermissionDenied`] when the destination is not writable.
    /// - [`confit_core::error::FilesystemError::FileUnknown`] when the destination is absent and the rename fails.
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

/// Returns the folder that holds the scratch entry.
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
    fn staged_write_round_trips_the_bytes() {
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
    fn staged_write_discards_the_pending_entry_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("atomic").join("note.bin");
        let outcome = StagedFile::write(&dest, |_| {
            Err(std::io::Error::other("scripted write failure"))
        });
        assert!(outcome.is_err(), "the staged write reports the write error");
        assert!(!dest.exists(), "the write error leaves no destination");
        let parent = dest.parent().unwrap().to_path_buf();
        let leftovers = read_dir(&parent).unwrap_or_default();
        assert!(
            leftovers.is_empty(),
            "the write error leaves no pending entry"
        );
    }

    #[test]
    fn staged_write_replaces_the_present_destination() {
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
