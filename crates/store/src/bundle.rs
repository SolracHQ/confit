//! Bundle
//!
//! Portable bundle archives holding manifests and blobs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::plan::Bundle;
use confit_core::progress::ProgressSender;

/// Portable bundle archive reads and writes.
///
/// Archives hold the manifest first with blob entries after.
pub trait BundleStore {
    /// Writes one portable bundle holding manifest and blobs.
    ///
    /// # Errors
    ///
    /// Compression and write failures surface as plan errors.
    fn write(&self, bundle: &Bundle, dest: &Path, progress: Option<&ProgressSender>) -> Result<()>;

    /// Reads one portable bundle into a live bundle.
    ///
    /// # Errors
    ///
    /// Unreadable files and bad payloads fail as plan errors.
    fn read(&self, path: &Path) -> Result<Bundle>;
}

/// Memory bundle store for tests.
///
/// Bundles ride an in-memory map keyed by destination path.
#[derive(Debug, Default)]
pub struct MemoryBundleStore {
    bundles: Mutex<HashMap<PathBuf, Bundle>>,
}

impl MemoryBundleStore {
    /// Builds an empty memory bundle store.
    pub fn new() -> Self {
        Self {
            bundles: Mutex::new(HashMap::new()),
        }
    }
}

impl BundleStore for MemoryBundleStore {
    fn write(&self, bundle: &Bundle, dest: &Path, progress: Option<&ProgressSender>) -> Result<()> {
        let _ = progress;
        let mut guard = match self.bundles.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(dest.to_path_buf(), bundle.clone());
        Ok(())
    }

    fn read(&self, path: &Path) -> Result<Bundle> {
        let guard = match self.bundles.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(path) {
            Some(bundle) => Ok(bundle.clone()),
            None => Err(Error::Plan(format!(
                "read bundle '{}': missing file",
                path.display()
            ))),
        }
    }
}
