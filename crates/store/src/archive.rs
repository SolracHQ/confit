//! Archive
//!
//! Member listing and extraction for compressed archives.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::error::{Error, Result};

/// Compressed archive member listing and extraction.
///
/// Member names read archive-relative with forward slashes.
pub trait ArchiveStore {
    /// Lists member names without reading content.
    ///
    /// # Errors
    ///
    /// Unreadable archives fail as plan errors.
    fn members(&self, archive: &Path) -> Result<Vec<String>>;

    /// Unpacks one archive once into the destination folder.
    ///
    /// Present folders skip, so repeated unpacks share bytes.
    ///
    /// # Errors
    ///
    /// Unreadable archives and write failures fail as plan errors.
    fn extract(&self, archive: &Path, dest: &Path) -> Result<PathBuf>;

    /// Reads one member through transparent decompression.
    ///
    /// # Errors
    ///
    /// Unknown members fail as plan errors naming the member.
    fn open_decompressed(&self, archive: &Path, member: &str) -> Result<Vec<u8>>;
}

/// Memory archive store for tests.
///
/// Members ride canned lists keyed by archive path.
#[derive(Debug, Default)]
pub struct MemoryArchiveStore {
    members: Mutex<HashMap<PathBuf, Vec<String>>>,
    contents: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

impl MemoryArchiveStore {
    /// Builds an empty memory archive store.
    pub fn new() -> Self {
        Self {
            members: Mutex::new(HashMap::new()),
            contents: Mutex::new(HashMap::new()),
        }
    }

    /// Seeds one canned member behind an archive path.
    pub fn insert(&self, archive: &Path, member: &str, bytes: &[u8]) {
        let mut members = match self.members.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let entry = members.entry(archive.to_path_buf()).or_default();
        if !entry.iter().any(|held| held == member) {
            entry.push(member.to_string());
        }
        let mut contents = match self.contents.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        contents.insert(archive.join(member), bytes.to_vec());
    }
}

impl ArchiveStore for MemoryArchiveStore {
    fn members(&self, archive: &Path) -> Result<Vec<String>> {
        let guard = match self.members.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(archive) {
            Some(members) => Ok(members.clone()),
            None => Err(Error::Plan(format!(
                "cannot read '{}': missing archive",
                archive.display()
            ))),
        }
    }

    fn extract(&self, archive: &Path, dest: &Path) -> Result<PathBuf> {
        let _ = archive;
        Ok(dest.to_path_buf())
    }

    fn open_decompressed(&self, archive: &Path, member: &str) -> Result<Vec<u8>> {
        let guard = match self.contents.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(&archive.join(member)) {
            Some(bytes) => Ok(bytes.clone()),
            None => Err(Error::Plan(format!(
                "cannot unpack '{}': missing member '{member}'",
                archive.display()
            ))),
        }
    }
}
