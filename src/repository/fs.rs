//! Filesystem
//!
//! Live filesystem backend for the filesystem seam.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

use super::traits::Filesystem;

/// Creates a link at a path pointing at a target.
///
/// Unix exposes one link call; Windows splits file links from dir links,
/// so the Windows branch links dirs as dirs and everything else as files.
#[cfg(unix)]
fn symlink(target: &str, path: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, path)
}

/// Creates a link at a path pointing at a target.
///
/// Unix exposes one link call; Windows splits file links from dir links,
/// so the Windows branch links dirs as dirs and everything else as files.
#[cfg(windows)]
fn symlink(target: &str, path: &Path) -> std::io::Result<()> {
    let target_path = Path::new(target);
    let resolved = match (target_path.is_absolute(), path.parent()) {
        (true, _) => target_path.to_path_buf(),
        (false, Some(parent)) => parent.join(target_path),
        (false, None) => target_path.to_path_buf(),
    };
    if resolved.is_dir() {
        std::os::windows::fs::symlink_dir(target, path)
    } else {
        std::os::windows::fs::symlink_file(target, path)
    }
}

/// Live filesystem backend.
///
/// Reads map absent paths to `None`; writes create parent dirs as needed.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFilesystem;

impl Filesystem for OsFilesystem {
    /// Reads raw bytes at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under read.
    ///
    /// # Returns
    ///
    /// File bytes, holding `None` for an absent path.
    ///
    /// # Errors
    ///
    /// Storage failure reading present bytes.
    fn read_bytes(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match fs::read(path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Store(e.to_string())),
        }
    }

    /// Reads text at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under read.
    ///
    /// # Returns
    ///
    /// File text, holding `None` for an absent path.
    ///
    /// # Errors
    ///
    /// Storage failure reading present text.
    fn read_string(&self, path: &str) -> Result<Option<String>> {
        match fs::read_to_string(path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Store(e.to_string())),
        }
    }

    /// Publishes bytes at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `bytes` - the payload for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing bytes.
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| Error::Store(e.to_string()))?;
        }
        fs::write(path, bytes).map_err(|e| Error::Store(e.to_string()))
    }

    /// Publishes text at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `text` - the text for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing text.
    fn write_string(&self, path: &Path, text: &str) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| Error::Store(e.to_string()))?;
        }
        fs::write(path, text).map_err(|e| Error::Store(e.to_string()))
    }

    /// Publishes bytes atomically at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `bytes` - the payload for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing bytes through temp file plus rename.
    fn write_bytes_tmp(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| Error::Store(e.to_string()))?;
        }
        let tmp = PathBuf::from(format!("{}.tmp-{}", path.display(), std::process::id()));
        let result: Result<()> = (|| {
            fs::write(&tmp, bytes).map_err(|e| Error::Store(e.to_string()))?;
            fs::rename(&tmp, path).map_err(|e| Error::Store(e.to_string()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    /// Reads a link target at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under read.
    ///
    /// # Returns
    ///
    /// Link target bytes, holding `None` for an absent path.
    ///
    /// # Errors
    ///
    /// Storage failure reading a present link.
    fn read_link(&self, path: &str) -> Result<Option<Vec<u8>>> {
        match fs::read_link(path).map(|target| target.as_os_str().as_encoded_bytes().to_vec()) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Error::Store(e.to_string())),
        }
    }

    /// Publishes a link at a path.
    ///
    /// # Arguments
    ///
    /// * `path` - the filesystem path under write.
    /// * `target` - the link target for the path.
    ///
    /// # Errors
    ///
    /// Storage failure publishing the link.
    fn write_link(&self, path: &Path, target: &str) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| Error::Store(e.to_string()))?;
        }
        symlink(target, path).map_err(|e| Error::Store(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stray_tmp_files(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(std::result::Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().contains(".tmp-"))
            })
            .collect()
    }

    #[test]
    fn bytes_round_trip_through_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("nested").join("plan.json");
        let bytes = b"{\"profile\":\"desktop\"}";
        OsFilesystem.write_bytes_tmp(&dest, bytes).unwrap();
        let path = dest.to_str().unwrap();
        let back = OsFilesystem.read_bytes(path).unwrap().unwrap();
        assert_eq!(back.as_slice(), bytes);
        assert!(stray_tmp_files(&dir.path().join("nested")).is_empty());
    }

    #[test]
    fn bytes_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("a").join("b").join("plan.toml");
        OsFilesystem
            .write_bytes(&dest, b"profile = \"desktop\"\n")
            .unwrap();
        let text = fs::read_to_string(&dest).unwrap();
        let value: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(value["profile"].as_str().unwrap(), "desktop");
    }

    #[test]
    fn read_missing_reads_none() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.txt").to_str().unwrap().to_string();
        assert!(OsFilesystem.read_bytes(&missing).unwrap().is_none());
        assert!(OsFilesystem.read_string(&missing).unwrap().is_none());
        assert!(OsFilesystem.read_link(&missing).unwrap().is_none());
    }

    #[test]
    fn read_present_returns_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("present.txt");
        fs::write(&file, b"hello").unwrap();
        let path = file.to_str().unwrap();
        assert_eq!(OsFilesystem.read_bytes(path).unwrap().unwrap(), b"hello");
        assert_eq!(OsFilesystem.read_string(path).unwrap().unwrap(), "hello");
    }

    #[test]
    fn read_link_returns_target_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("link");
        OsFilesystem.write_link(&link, "/tmp/target").unwrap();
        let path = link.to_str().unwrap();
        assert_eq!(
            OsFilesystem.read_link(path).unwrap().unwrap(),
            b"/tmp/target"
        );
    }

    #[test]
    fn write_plus_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let bytes_path = dir.path().join("data.bin");
        OsFilesystem.write_bytes(&bytes_path, b"bytes").unwrap();
        let key = bytes_path.to_str().unwrap();
        assert_eq!(OsFilesystem.read_bytes(key).unwrap().unwrap(), b"bytes");
        let text_path = dir.path().join("data.txt");
        OsFilesystem.write_string(&text_path, "text").unwrap();
        let key = text_path.to_str().unwrap();
        assert_eq!(OsFilesystem.read_string(key).unwrap().unwrap(), "text");
    }

    #[test]
    fn write_tmp_round_trips_without_stray() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("plan.json");
        OsFilesystem.write_bytes_tmp(&dest, b"{}").unwrap();
        let key = dest.to_str().unwrap();
        assert_eq!(OsFilesystem.read_bytes(key).unwrap().unwrap(), b"{}");
        assert!(stray_tmp_files(dir.path()).is_empty());
    }

    #[test]
    fn directory_read_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(OsFilesystem.read_bytes(path).is_err());
        assert!(OsFilesystem.read_string(path).is_err());
    }

    #[test]
    fn invalid_utf8_read_errors() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("bad.txt");
        fs::write(&file, [0xff, 0xfe]).unwrap();
        let path = file.to_str().unwrap();
        assert!(OsFilesystem.read_string(path).is_err());
        assert!(OsFilesystem.read_bytes(path).unwrap().is_some());
    }
}
