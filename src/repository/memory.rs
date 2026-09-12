//! Memory
//!
//! In-memory backend for the filesystem seam.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use crate::error::{Error, Result};

use super::traits::Filesystem;

/// In-memory filesystem backend for tests.
///
/// Holds file bytes keyed by path; `failures` forces read errors ahead of stored bytes.
#[derive(Debug, Default)]
pub struct MemoryFilesystem {
    /// Holds path to file bytes.
    pub files: RefCell<BTreeMap<String, Vec<u8>>>,
    /// Holds path to read failure reason.
    pub failures: RefCell<BTreeMap<String, String>>,
}

impl Filesystem for MemoryFilesystem {
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
    /// Storage failure for a path holding a forced failure.
    fn read_bytes(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if let Some(reason) = self.failures.borrow().get(path) {
            return Err(Error::Store(reason.clone()));
        }
        Ok(self.files.borrow().get(path).cloned())
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
    /// Storage failure for forced failures or invalid UTF-8.
    fn read_string(&self, path: &str) -> Result<Option<String>> {
        if let Some(reason) = self.failures.borrow().get(path) {
            return Err(Error::Store(reason.clone()));
        }
        match self.files.borrow().get(path) {
            None => Ok(None),
            Some(bytes) => match String::from_utf8(bytes.clone()) {
                Ok(text) => Ok(Some(text)),
                Err(e) => Err(Error::Store(format!("invalid utf-8: {e}"))),
            },
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
    /// Success holds for every call.
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.files
            .borrow_mut()
            .insert(path.display().to_string(), bytes.to_vec());
        Ok(())
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
    /// Success holds for every call.
    fn write_string(&self, path: &Path, text: &str) -> Result<()> {
        self.files
            .borrow_mut()
            .insert(path.display().to_string(), text.as_bytes().to_vec());
        Ok(())
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
    /// Success holds for every call.
    fn write_bytes_tmp(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.files
            .borrow_mut()
            .insert(path.display().to_string(), bytes.to_vec());
        Ok(())
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
    /// Storage failure for a path holding a forced failure.
    fn read_link(&self, path: &str) -> Result<Option<Vec<u8>>> {
        if let Some(reason) = self.failures.borrow().get(path) {
            return Err(Error::Store(reason.clone()));
        }
        Ok(self.files.borrow().get(path).cloned())
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
    /// Success holds for every call.
    fn write_link(&self, path: &Path, target: &str) -> Result<()> {
        self.files
            .borrow_mut()
            .insert(path.display().to_string(), target.as_bytes().to_vec());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_plus_read_round_trips_bytes() {
        let fs = MemoryFilesystem {
            files: RefCell::new([("/tmp/a".to_string(), b"data".to_vec())].into()),
            failures: RefCell::new(BTreeMap::new()),
        };
        assert_eq!(fs.read_bytes("/tmp/a").unwrap().unwrap(), b"data");
    }

    #[test]
    fn insert_plus_read_round_trips_strings() {
        let fs = MemoryFilesystem {
            files: RefCell::new([("/tmp/a".to_string(), b"hello".to_vec())].into()),
            failures: RefCell::new(BTreeMap::new()),
        };
        assert_eq!(fs.read_string("/tmp/a").unwrap().unwrap(), "hello");
    }

    #[test]
    fn failures_win_over_files() {
        let fs = MemoryFilesystem {
            files: RefCell::new([("/tmp/locked".to_string(), b"data".to_vec())].into()),
            failures: RefCell::new([("/tmp/locked".to_string(), "denied".to_string())].into()),
        };
        let err = fs.read_bytes("/tmp/locked").unwrap_err();
        assert!(err.to_string().contains("denied"));
        let err = fs.read_string("/tmp/locked").unwrap_err();
        assert!(err.to_string().contains("denied"));
        let err = fs.read_link("/tmp/locked").unwrap_err();
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn missing_reads_none() {
        let fs = MemoryFilesystem::default();
        assert!(fs.read_bytes("/tmp/missing").unwrap().is_none());
        assert!(fs.read_string("/tmp/missing").unwrap().is_none());
        assert!(fs.read_link("/tmp/missing").unwrap().is_none());
    }

    #[test]
    fn write_tmp_stores() {
        let fs = MemoryFilesystem::default();
        fs.write_bytes_tmp(Path::new("/tmp/plan"), b"bytes")
            .unwrap();
        assert_eq!(fs.read_bytes("/tmp/plan").unwrap().unwrap(), b"bytes");
    }

    #[test]
    fn write_plus_read_round_trips() {
        let fs = MemoryFilesystem::default();
        fs.write_bytes(Path::new("/tmp/a"), b"data").unwrap();
        assert_eq!(fs.read_bytes("/tmp/a").unwrap().unwrap(), b"data");
        fs.write_string(Path::new("/tmp/b"), "hello").unwrap();
        assert_eq!(fs.read_string("/tmp/b").unwrap().unwrap(), "hello");
        fs.write_link(Path::new("/tmp/c"), "/tmp/target").unwrap();
        assert_eq!(fs.read_link("/tmp/c").unwrap().unwrap(), b"/tmp/target");
    }

    #[test]
    fn invalid_utf8_errors() {
        let fs = MemoryFilesystem {
            files: RefCell::new([("/tmp/bad".to_string(), vec![0xff, 0xfe])].into()),
            failures: RefCell::new(BTreeMap::new()),
        };
        let err = fs.read_string("/tmp/bad").unwrap_err();
        assert!(err.to_string().contains("invalid utf-8"));
    }
}
