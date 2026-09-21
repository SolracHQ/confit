//! Fs
//!
//! Effect seam for file reads and writes.

use std::path::{Path, PathBuf};

pub mod memory;
pub mod snapshot;

/// Read chunk size for streaming hashes.
const STREAM_BUF_BYTES: usize = 8 * 1024;

/// Filesystem backend.
pub trait Filesystem {
    /// Reads raw bytes from a path.
    ///
    /// # Errors
    ///
    /// Missing files and permission failures surface as io errors.
    fn read(&self, path: &Path) -> std::io::Result<Vec<u8>>;

    /// Writes bytes to a path, creating parents as needed.
    ///
    /// # Errors
    ///
    /// Missing parents and permission failures surface as io errors.
    fn write(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;

    /// Sets unix permission bits on a path.
    ///
    /// Runs after writes for documents carrying a mode.
    /// Paths without a recorded mode keep the umask default.
    ///
    /// # Errors
    ///
    /// Missing paths and permission failures surface as io errors.
    fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()>;

    /// Creates a symlink at `link` pointing at `target`.
    ///
    /// Parents build on demand. Present files at `link` yield.
    ///
    /// # Errors
    ///
    /// Missing parents and permission failures surface as io errors.
    fn symlink(&self, link: &Path, target: &Path) -> std::io::Result<()>;

    /// Lists immediate children of a directory as full paths.
    ///
    /// # Errors
    ///
    /// Missing directories and permission failures surface as io errors.
    fn list_dir(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>>;

    /// Removes one file or symlink path.
    ///
    /// # Errors
    ///
    /// Missing paths and permission failures surface as io errors.
    fn remove(&self, path: &Path) -> std::io::Result<()>;

    /// Reads one symlink target without following it.
    ///
    /// Plain files and missing paths read as `None`.
    fn read_link(&self, path: &Path) -> Option<PathBuf>;

    /// Reads unix permission bits without following content.
    ///
    /// Symlinks and missing paths read as `None`.
    fn file_mode(&self, path: &Path) -> Option<u32>;

    /// Reports path presence.
    fn exists(&self, path: &Path) -> bool;

    /// Opens a streaming reader for a path.
    ///
    /// Readers honor `read` link handling on each backend.
    ///
    /// # Errors
    ///
    /// Missing paths and permission failures surface as io errors.
    fn reader(&self, path: &Path) -> std::io::Result<Box<dyn std::io::Read>>;

    /// Opens a streaming writer for a path, creating parents as needed.
    ///
    /// Writers replace links like `write` does.
    ///
    /// # Errors
    ///
    /// Missing parents and permission failures surface as io errors.
    fn writer(&self, path: &Path) -> std::io::Result<Box<dyn std::io::Write + '_>>;

    /// Reports the byte length `read` would return for a path.
    ///
    /// Links measure their raw target text without following it.
    ///
    /// # Errors
    ///
    /// Missing paths and permission failures surface as io errors.
    fn file_len(&self, path: &Path) -> std::io::Result<u64>;

    /// Copies one path to another through streams, reporting bytes moved.
    ///
    /// # Errors
    ///
    /// Missing sources and unwritable destinations surface as io errors.
    fn copy(&self, from: &Path, to: &Path) -> std::io::Result<u64> {
        let mut reader = self.reader(from)?;
        let mut writer = self.writer(to)?;
        let len = std::io::copy(&mut reader, &mut writer)?;
        writer.flush()?;
        Ok(len)
    }

    /// Hashes one path with sha256 through a stream.
    ///
    /// Reports the lowercase hex digest and the hashed byte count.
    ///
    /// # Errors
    ///
    /// Missing paths and permission failures surface as io errors.
    fn hash_file(&self, path: &Path) -> std::io::Result<(String, u64)> {
        use sha2::Digest as _;

        let mut reader = self.reader(path)?;
        let mut hasher = sha2::Sha256::new();
        let mut len = 0u64;
        let mut buf = [0u8; STREAM_BUF_BYTES];
        loop {
            let read = std::io::Read::read(&mut reader, &mut buf)?;
            if read == 0 {
                break;
            }
            hasher.update(&buf[..read]);
            len += read as u64;
        }
        let hex: String = hasher
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        Ok((hex, len))
    }
}
