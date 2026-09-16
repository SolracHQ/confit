//! Fs
//!
//! Filesystem seam over host disk plus memory fakes for tests.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ids::{DocPath, ReadOutcome};

/// Filesystem backend behind plan reads plus apply writes.
///
/// Tests run against memory. The binary runs against the host disk.
///
/// # Examples
///
/// ```text
/// use confit_core::fs::{Filesystem, OsFs};
///
/// let fs = OsFs;
/// assert!(matches!(fs.exists(std::path::Path::new("/definitely-missing-confit-path")), false));
/// ```
pub trait Filesystem {
    /// Reads raw bytes from a path.
    ///
    /// # Errors
    ///
    /// Missing files plus permission failures surface as io errors.
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;

    /// Writes bytes to a path, creating parents as needed.
    ///
    /// # Errors
    ///
    /// Missing parents plus permission failures surface as io errors.
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;

    /// Sets unix permission bits on a path.
    ///
    /// Runs after writes for documents carrying a mode.
    /// Paths without a recorded mode keep the umask default.
    ///
    /// # Errors
    ///
    /// Missing paths plus permission failures surface as io errors.
    fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()>;

    /// Creates a symlink at `link` pointing at `target`.
    ///
    /// Parents build on demand. Present files at `link` yield.
    ///
    /// # Errors
    ///
    /// Missing parents plus permission failures surface as io errors.
    fn symlink(&self, link: &Path, target: &Path) -> std::io::Result<()>;

    /// Lists immediate children of a directory as full paths.
    ///
    /// # Errors
    ///
    /// Missing directories plus permission failures surface as io errors.
    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>>;

    /// Removes one file or symlink path.
    ///
    /// # Errors
    ///
    /// Missing paths plus permission failures surface as io errors.
    fn remove(&self, path: &Path) -> std::io::Result<()>;

    /// Reads one symlink target without following it.
    ///
    /// Plain files plus missing paths read as `None`.
    fn read_link(&self, path: &Path) -> Option<PathBuf>;

    /// Reads unix permission bits without following content.
    ///
    /// Symlinks plus missing paths read as `None`.
    fn file_mode(&self, path: &Path) -> Option<u32>;

    /// Reports path presence.
    fn exists(&self, path: &Path) -> bool;
}

/// Host filesystem backend.
///
/// # Examples
///
/// ```text
/// use confit_core::fs::{Filesystem, OsFs};
///
/// let fs = OsFs;
/// assert!(matches!(fs.exists(std::path::Path::new("/definitely-missing-confit-path")), false));
/// ```
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
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
    ///
    /// let outcome = OsFs.read(std::path::Path::new("/definitely-missing-confit-path"));
    /// assert!(matches!(outcome, Err(_)));
    /// ```
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
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
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
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
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
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
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
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
    ///
    /// let outcome = OsFs.list_dir(std::path::Path::new("/definitely-missing-confit-path"));
    /// assert!(matches!(outcome, Err(_)));
    /// ```
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
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
    ///
    /// let outcome = OsFs.remove(std::path::Path::new("/definitely-missing-confit-path"));
    /// assert!(matches!(outcome, Err(_)));
    /// ```
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
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
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
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
    ///
    /// assert!(matches!(OsFs.file_mode(std::path::Path::new("/definitely-missing-confit-path")), None));
    /// ```
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
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, OsFs};
    ///
    /// assert!(matches!(OsFs.exists(std::path::Path::new("/definitely-missing-confit-path")), false));
    /// ```
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// In-memory filesystem backend for tests.
///
/// Tests never touch home folders through this fake.
///
/// # Examples
///
/// ```text
/// use confit_core::fs::{Filesystem, MemoryFs};
///
/// let fs = MemoryFs::new();
/// assert!(matches!(fs.write(std::path::Path::new("note"), b"hi"), Ok(())));
/// assert!(matches!(fs.read(std::path::Path::new("note")), Ok(_)));
/// ```
#[derive(Debug, Default)]
pub struct MemoryFs {
    /// Files by path.
    files: std::cell::RefCell<HashMap<PathBuf, Vec<u8>>>,
    /// Symlink targets by link path.
    links: std::cell::RefCell<HashMap<PathBuf, PathBuf>>,
    /// Unix permission bits by path.
    modes: std::cell::RefCell<HashMap<PathBuf, u32>>,
    /// Paths failing reads with permission errors.
    unreadable: HashSet<PathBuf>,
}

impl MemoryFs {
    /// Builds an empty memory backend.
    ///
    /// # Returns
    ///
    /// The backend holding no files.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, MemoryFs};
    ///
    /// let fs = MemoryFs::new();
    /// assert!(!fs.exists(std::path::Path::new("note")));
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks one path as failing reads with a permission error.
    ///
    /// # Arguments
    ///
    /// * `path` - the path failing future reads.
    pub fn mark_unreadable(&mut self, path: &Path) {
        self.unreadable.insert(path.to_path_buf());
    }
}

impl Filesystem for MemoryFs {
    /// Reads bytes from a memory path.
    ///
    /// Link paths read as their raw target text, matching
    /// the drift shape for link documents.
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
    /// Missing paths fail as not found. Marked paths fail as denied.
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        if self.unreadable.contains(path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "permission denied",
            ));
        }
        if let Some(target) = self.links.borrow().get(path) {
            return Ok(target.as_os_str().as_encoded_bytes().to_vec());
        }
        self.files.borrow().get(path).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no such file: {}", path.display()),
            )
        })
    }

    /// Writes bytes to a memory path.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under writing.
    /// * `bytes` - the bytes landing in memory.
    ///
    /// # Returns
    ///
    /// Unit once the bytes land.
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.links.borrow_mut().remove(path);
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    /// Records unix permission bits on a memory path.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under updating.
    /// * `mode` - the unix permission bits held in memory.
    ///
    /// # Returns
    ///
    /// Unit once the bits land.
    fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        self.modes.borrow_mut().insert(path.to_path_buf(), mode);
        Ok(())
    }

    /// Records a memory symlink, replacing present files.
    ///
    /// # Arguments
    ///
    /// * `link` - the symlink path under writing.
    /// * `target` - the raw target text the link holds.
    ///
    /// # Returns
    ///
    /// Unit once the link lands.
    fn symlink(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        self.files.borrow_mut().remove(link);
        self.modes.borrow_mut().remove(link);
        self.links
            .borrow_mut()
            .insert(link.to_path_buf(), target.to_path_buf());
        Ok(())
    }

    /// Lists memory children with a matching parent path.
    ///
    /// Missing folders read as empty on memory backends.
    ///
    /// # Arguments
    ///
    /// * `dir` - the folder under listing.
    ///
    /// # Returns
    ///
    /// Full child paths in sorted order.
    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut out: Vec<PathBuf> = self
            .files
            .borrow()
            .keys()
            .chain(self.links.borrow().keys())
            .filter(|path| path.parent() == Some(dir))
            .cloned()
            .collect();
        out.sort();
        Ok(out)
    }

    /// Removes one memory file or symlink path.
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
    /// Missing paths fail as not found.
    fn remove(&self, path: &Path) -> std::io::Result<()> {
        let file = self.files.borrow_mut().remove(path);
        let link = self.links.borrow_mut().remove(path);
        self.modes.borrow_mut().remove(path);
        if file.is_some() || link.is_some() {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("no such file: {}", path.display()),
            ))
        }
    }

    /// Reads one memory symlink target without following it.
    ///
    /// Marked paths read as `None`, keeping denies on reads.
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
    /// ```text
    /// use confit_core::fs::{Filesystem, MemoryFs};
    ///
    /// let fs = MemoryFs::new();
    /// assert!(matches!(fs.read_link(std::path::Path::new("note")), None));
    /// ```
    fn read_link(&self, path: &Path) -> Option<PathBuf> {
        if self.unreadable.contains(path) {
            return None;
        }
        self.links.borrow().get(path).cloned()
    }

    /// Reads recorded permission bits from a memory path.
    ///
    /// # Arguments
    ///
    /// * `path` - the path under reading.
    ///
    /// # Returns
    ///
    /// The recorded bits for files, else `None`.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::fs::{Filesystem, MemoryFs};
    ///
    /// let fs = MemoryFs::new();
    /// assert!(matches!(fs.file_mode(std::path::Path::new("note")), None));
    /// ```
    fn file_mode(&self, path: &Path) -> Option<u32> {
        self.modes.borrow().get(path).copied()
    }

    /// Reports memory path presence.
    ///
    /// Links count as present beside files.
    ///
    /// # Arguments
    ///
    /// * `path` - the path under testing.
    ///
    /// # Returns
    ///
    /// True while the path holds bytes or a link.
    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path) || self.links.borrow().contains_key(path)
    }
}

/// Snapshots one document path through a backend.
///
/// Links snapshot as their raw target text, matching the drift
/// shape for link documents without following the link.
/// Permission bits ride along for mode comparisons.
///
/// # Arguments
///
/// * `path` - the document path with a leading tilde for home targets.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Absent for missing paths, present bytes plus mode for
/// readable files, unreadable holding the failure detail otherwise.
///
/// # Examples
///
/// ```text
/// use confit_core::fs::{OsFs, snapshot};
/// use confit_core::ids::DocPath;
///
/// let outcome = snapshot(&DocPath::new("/definitely-missing-confit-path"), &OsFs);
/// assert!(matches!(outcome, confit_core::ids::ReadOutcome::Absent));
/// ```
pub fn snapshot(path: &DocPath, fs: &dyn Filesystem) -> ReadOutcome {
    let expanded = path.expand();
    if let Some(target) = fs.read_link(&expanded) {
        return ReadOutcome::Present {
            bytes: target.as_os_str().as_encoded_bytes().to_vec(),
            mode: None,
        };
    }
    match fs.read(&expanded) {
        Ok(bytes) => ReadOutcome::Present {
            bytes,
            mode: fs.file_mode(&expanded),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ReadOutcome::Absent,
        Err(error) => ReadOutcome::Unreadable {
            reason: error.to_string(),
        },
    }
}
