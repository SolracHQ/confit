//! Atomic file writes through staged sibling temp files.

use std::path::{Path, PathBuf};

use crate::fs::create_parent;

/// Staged scratch file under one destination folder.
///
/// Drop removes the scratch file. Persist moves it onto the destination.
pub struct StagedFile {
    /// The staged scratch file.
    file: tempfile::NamedTempFile,
}

impl StagedFile {
    /// Stage one empty scratch file under the destination folder.
    ///
    /// - Parent folders build first.
    ///
    /// # Errors
    ///
    /// - [NotFound] for missing parents.
    /// - [PermissionDenied] for unwritable destinations.
    /// - [Other] for scratch staging failures.
    pub fn stage(dest: &Path) -> std::io::Result<Self> {
        create_parent(dest)?;
        tempfile::NamedTempFile::new_in(scratch_dir(dest)).map(|file| Self { file })
    }

    /// Write one file atomically through a sibling rename.
    ///
    /// - Parent folders build first.
    /// - The closure writes the open scratch file; failures remove it and
    ///   report the closure error.
    /// - Rename failures with a present destination report success with the
    ///   present entry kept.
    ///
    /// # Errors
    ///
    /// - [NotFound] for missing parents.
    /// - [PermissionDenied] for unwritable destinations.
    /// - [Other] for staging failures.
    pub fn write(
        dest: &Path,
        write: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
    ) -> std::io::Result<()> {
        let mut staged = Self::stage(dest)?;
        write(staged.file.as_file_mut())?;
        staged.persist(dest)
    }

    /// Return the scratch path for direct writes.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Move the scratch file onto the destination.
    ///
    /// - A present destination stays on persist failures.
    ///
    /// # Errors
    ///
    /// - [PermissionDenied] for unwritable destinations.
    /// - [Other] for persist failures with an absent destination.
    pub fn persist(self, dest: &Path) -> std::io::Result<()> {
        match self.file.persist(dest) {
            Ok(_) => Ok(()),
            Err(_) if dest.exists() => Ok(()),
            Err(error) => Err(error.error),
        }
    }
}

/// Staged scratch dir under one destination folder.
///
/// Drop removes the scratch dir. Persist renames it onto the destination.
pub struct StagedDir {
    /// The staged scratch dir.
    dir: tempfile::TempDir,
}

impl StagedDir {
    /// Stage one empty scratch dir under the destination folder.
    ///
    /// - Parent folders build first.
    ///
    /// # Errors
    ///
    /// - [NotFound] for missing parents.
    /// - [PermissionDenied] for unwritable destinations.
    /// - [Other] for scratch staging failures.
    pub fn stage(dest: &Path) -> std::io::Result<Self> {
        create_parent(dest)?;
        tempfile::TempDir::new_in(scratch_dir(dest)).map(|dir| Self { dir })
    }

    /// Return the scratch path for direct writes.
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Rename the scratch dir onto the destination.
    ///
    /// - A present destination stays on rename failures.
    ///
    /// # Errors
    ///
    /// - [NotFound] for missing scratch entries.
    /// - [PermissionDenied] for unwritable destinations.
    /// - [Other] for rename failures with an absent destination.
    pub fn persist(self, dest: &Path) -> std::io::Result<()> {
        match std::fs::rename(self.path(), dest) {
            Ok(()) => {
                let _ = self.dir.keep();
                Ok(())
            }
            Err(_) if dest.exists() => Ok(()),
            Err(error) => Err(error),
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
