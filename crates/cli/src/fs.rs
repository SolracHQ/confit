//! Fs
//!
//! Filesystem seam over host disk plus memory fakes for tests.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use confit_core::ids::{DocPath, ReadOutcome};

/// Filesystem backend behind plan reads plus apply writes.
///
/// Tests run against memory. The binary runs against the host disk.
///
/// # Examples
///
/// ```rust
/// use confit_cli::fs::{Filesystem, OsFs};
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

    /// Reports path presence.
    fn exists(&self, path: &Path) -> bool;
}

/// Host filesystem backend.
///
/// # Examples
///
/// ```rust
/// use confit_cli::fs::{Filesystem, OsFs};
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
    /// ```rust
    /// use confit_cli::fs::{Filesystem, OsFs};
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
    /// ```rust
    /// use confit_cli::fs::{Filesystem, OsFs};
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
    /// ```rust
    /// use confit_cli::fs::{Filesystem, OsFs};
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
/// ```rust
/// use confit_cli::fs::{Filesystem, MemoryFs};
///
/// let fs = MemoryFs::new();
/// assert!(matches!(fs.write(std::path::Path::new("note"), b"hi"), Ok(())));
/// assert!(matches!(fs.read(std::path::Path::new("note")), Ok(_)));
/// ```
#[derive(Debug, Default)]
pub struct MemoryFs {
    /// Files by path.
    files: std::cell::RefCell<HashMap<PathBuf, Vec<u8>>>,
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
    /// ```rust
    /// use confit_cli::fs::{Filesystem, MemoryFs};
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
        self.files
            .borrow_mut()
            .insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    /// Reports memory path presence.
    ///
    /// # Arguments
    ///
    /// * `path` - the path under testing.
    ///
    /// # Returns
    ///
    /// True while the path holds bytes.
    fn exists(&self, path: &Path) -> bool {
        self.files.borrow().contains_key(path)
    }
}

/// Snapshots one document path through a backend.
///
/// # Arguments
///
/// * `path` - the document path with a leading tilde for home targets.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Absent for missing paths, present bytes for readable files,
/// unreadable holding the failure detail otherwise.
///
/// # Examples
///
/// ```rust
/// use confit_cli::fs::{OsFs, snapshot};
/// use confit_core::ids::DocPath;
///
/// let outcome = snapshot(&DocPath::new("/definitely-missing-confit-path"), &OsFs);
/// assert!(matches!(outcome, confit_core::ids::ReadOutcome::Absent));
/// ```
pub fn snapshot(path: &DocPath, fs: &dyn Filesystem) -> ReadOutcome {
    let expanded = path.expand();
    match fs.read(&expanded) {
        Ok(bytes) => ReadOutcome::Present(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ReadOutcome::Absent,
        Err(error) => ReadOutcome::Unreadable {
            reason: error.to_string(),
        },
    }
}
