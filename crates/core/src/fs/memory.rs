//! Memory
//!
//! In-memory filesystem backend for tests.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::Filesystem;

/// In-memory filesystem backend for tests.
///
/// Tests never touch home folders through this fake.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::Filesystem;
/// use confit_core::fs::memory::MemoryFs;
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
    /// Opens a cursor over the bytes `read` would return.
    ///
    /// Unreadable paths fail as denied, links read as target
    /// text, missing paths fail as not found.
    ///
    /// # Errors
    ///
    /// Missing paths fail as not found. Marked paths fail as denied.
    fn reader(&self, path: &Path) -> std::io::Result<Box<dyn std::io::Read>> {
        let bytes = self.read(path)?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }

    /// Buffers a memory write, committing the bytes into the map on drop.
    ///
    /// Dropped buffers replace links like `write` does.
    /// Dropped empty buffers land empty files.
    ///
    /// # Errors
    ///
    /// Memory writes never fail. The result only carries the handle.
    fn writer(&self, path: &Path) -> std::io::Result<Box<dyn std::io::Write + '_>> {
        Ok(Box::new(MemoryWriter {
            fs: self,
            path: path.to_path_buf(),
            buf: Vec::new(),
        }))
    }

    /// Reports the byte length `read` would return for a memory path.
    ///
    /// Links measure their raw target text without following it.
    ///
    /// # Errors
    ///
    /// Missing paths fail as not found. Marked paths fail as denied.
    fn file_len(&self, path: &Path) -> std::io::Result<u64> {
        if self.unreadable.contains(path) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "permission denied",
            ));
        }
        if let Some(target) = self.links.borrow().get(path) {
            return Ok(target.as_os_str().as_encoded_bytes().len() as u64);
        }
        self.files
            .borrow()
            .get(path)
            .map(|bytes| bytes.len() as u64)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("no such file: {}", path.display()),
                )
            })
    }
}

struct MemoryWriter<'a> {
    fs: &'a MemoryFs,
    path: PathBuf,
    buf: Vec<u8>,
}

impl std::io::Write for MemoryWriter<'_> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for MemoryWriter<'_> {
    fn drop(&mut self) {
        self.fs.links.borrow_mut().remove(&self.path);
        self.fs
            .files
            .borrow_mut()
            .insert(self.path.clone(), std::mem::take(&mut self.buf));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::sha256_hex;

    #[test]
    fn copy_roundtrips_bytes() {
        let fs = MemoryFs::new();
        let from = Path::new("src.bin");
        let to = Path::new("dst.bin");
        match fs.write(from, &[0xFF, 0x00, 0x41]) {
            Ok(()) => {}
            Err(error) => panic!("source writes: {error}"),
        }
        match fs.copy(from, to) {
            Ok(moved) => assert_eq!(moved, 3),
            Err(error) => panic!("copy runs: {error}"),
        }
        match fs.read(to) {
            Ok(bytes) => assert_eq!(bytes, vec![0xFF, 0x00, 0x41]),
            Err(error) => panic!("destination reads: {error}"),
        }
    }

    #[test]
    fn hash_file_matches_sha256_hex_and_counts_bytes() {
        let fs = MemoryFs::new();
        let path = Path::new("note.bin");
        match fs.write(path, b"abc") {
            Ok(()) => {}
            Err(error) => panic!("file writes: {error}"),
        }
        match fs.hash_file(path) {
            Ok((digest, len)) => {
                assert_eq!(digest, sha256_hex(b"abc"));
                assert_eq!(len, 3);
            }
            Err(error) => panic!("hash runs: {error}"),
        }
    }

    #[test]
    fn file_len_matches_content_and_missing_fails_not_found() {
        let fs = MemoryFs::new();
        let path = Path::new("note.bin");
        match fs.write(path, b"hello") {
            Ok(()) => {}
            Err(error) => panic!("file writes: {error}"),
        }
        match fs.file_len(path) {
            Ok(len) => assert_eq!(len, 5),
            Err(error) => panic!("length reads: {error}"),
        }
        match fs.file_len(Path::new("missing.bin")) {
            Ok(_) => panic!("missing length passes"),
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
        }
    }

    #[test]
    fn writer_drop_commits_bytes_for_read_back() {
        use std::io::Write as _;

        let fs = MemoryFs::new();
        let path = Path::new("note.bin");
        {
            let mut writer = match fs.writer(path) {
                Ok(writer) => writer,
                Err(error) => panic!("writer opens: {error}"),
            };
            match writer.write_all(b"hi") {
                Ok(()) => {}
                Err(error) => panic!("writer writes: {error}"),
            }
        }
        match fs.read(path) {
            Ok(bytes) => assert_eq!(bytes, b"hi"),
            Err(error) => panic!("written bytes read: {error}"),
        }
    }
}
