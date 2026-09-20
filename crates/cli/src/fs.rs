//! Fs
//!
//! Host filesystem backend. The seam lives in core,
//! the effect lives here beside its only callers.

use std::path::{Path, PathBuf};

use confit_core::fs::Filesystem;

/// Host filesystem backend.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFs;

impl Filesystem for OsFs {
    /// Reads raw bytes from a host path.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under reading.
    ///
    /// # Returns
    ///
    /// The file bytes.
    ///
    /// # Errors
    ///
    /// Missing files plus permission failures surface as io errors.
    ///
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    /// Writes bytes to a host path, creating parents as needed.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under writing.
    /// * `bytes` - the bytes landing on disk.
    ///
    /// # Returns
    ///
    /// Unit once the bytes land.
    ///
    /// # Errors
    ///
    /// Missing parents plus permission failures surface as io errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::fs::OsFs;
    /// use confit_core::fs::Filesystem;
    ///
    /// let dir = std::env::temp_dir().join("confit-fs-doc-example");
    /// std::fs::create_dir_all(&dir);
    /// let path = dir.join("note.txt");
    /// assert!(matches!(OsFs.write(&path, b"hi"), Ok(())));
    /// let _ = std::fs::remove_dir_all(&dir);
    /// ```
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, bytes)
    }

    /// Sets unix permission bits on a host path.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under updating.
    /// * `mode` - the unix permission bits landing on disk.
    ///
    /// # Returns
    ///
    /// Unit once the bits land.
    ///
    /// # Errors
    ///
    /// Missing paths plus permission failures surface as io errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::fs::OsFs;
    /// use confit_core::fs::Filesystem;
    ///
    /// let dir = std::env::temp_dir().join("confit-mode-doc-example");
    /// std::fs::create_dir_all(&dir);
    /// let path = dir.join("note.txt");
    /// assert!(matches!(OsFs.write(&path, b"hi"), Ok(())));
    /// assert!(matches!(OsFs.set_mode(&path, 0o644), Ok(())));
    /// let _ = std::fs::remove_dir_all(&dir);
    /// ```
    fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    /// Creates a host symlink, replacing present files.
    ///
    /// # Arguments
    ///
    /// * `link` - the symlink path under writing.
    /// * `target` - the raw target text the link holds.
    ///
    /// # Returns
    ///
    /// Unit once the link lands.
    ///
    /// # Errors
    ///
    /// Missing parents plus permission failures surface as io errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::fs::OsFs;
    /// use confit_core::fs::Filesystem;
    ///
    /// let dir = std::env::temp_dir().join("confit-symlink-doc-example");
    /// let link = dir.join("shortcut");
    /// assert!(matches!(OsFs.symlink(&link, std::path::Path::new("dest")), Ok(())));
    /// let _ = std::fs::remove_dir_all(&dir);
    /// ```
    fn symlink(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        if let Some(parent) = link.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        if link.symlink_metadata().is_ok() {
            std::fs::remove_file(link)?;
        }
        std::os::unix::fs::symlink(target, link)
    }

    /// Lists host directory children as full paths.
    ///
    /// # Arguments
    ///
    /// * `dir` - the folder under listing.
    ///
    /// # Returns
    ///
    /// Full child paths in directory order.
    ///
    /// # Errors
    ///
    /// Missing directories plus permission failures surface as io errors.
    ///
    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            out.push(entry?.path());
        }
        Ok(out)
    }

    /// Removes one host file or symlink path.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under removal.
    ///
    /// # Returns
    ///
    /// Unit once the path clears.
    ///
    /// # Errors
    ///
    /// Missing paths plus permission failures surface as io errors.
    ///
    fn remove(&self, path: &Path) -> std::io::Result<()> {
        std::fs::remove_file(path)
    }

    /// Reads one host symlink target without following it.
    ///
    /// # Arguments
    ///
    /// * `path` - the link path under reading.
    ///
    /// # Returns
    ///
    /// The raw target for links, else `None`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::fs::OsFs;
    /// use confit_core::fs::Filesystem;
    ///
    /// let dir = std::env::temp_dir().join("confit-readlink-doc-example");
    /// let link = dir.join("shortcut");
    /// assert!(matches!(OsFs.symlink(&link, std::path::Path::new("dest")), Ok(())));
    /// assert!(matches!(OsFs.read_link(&link), Some(_)));
    /// let _ = std::fs::remove_dir_all(&dir);
    /// ```
    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        std::fs::read_link(path).ok()
    }

    /// Reads unix permission bits from a host path.
    ///
    /// # Arguments
    ///
    /// * `path` - the path under reading.
    ///
    /// # Returns
    ///
    /// The permission bits for files, else `None` for
    /// symlinks plus missing paths.
    ///
    fn file_mode(&self, path: &Path) -> Option<u32> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::symlink_metadata(path).ok()?;
        if metadata.file_type().is_symlink() {
            return None;
        }
        Some(metadata.permissions().mode() & 0o777)
    }

    /// Reports host path presence.
    ///
    /// # Arguments
    ///
    /// * `path` - the path under testing.
    ///
    /// # Returns
    ///
    /// True while the path exists.
    ///
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}
