//! Memory
//!
//! Memory bundle store for tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::plan::Bundle;
use confit_core::progress::ProgressSender;

use super::BundleStore;

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
    fn write(
        &self,
        bundle: &Bundle,
        dest: &Path,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf> {
        let _ = progress;
        let dest = super::ensure_bundle_extension(dest);
        let mut guard = match self.bundles.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(dest.clone(), bundle.clone());
        Ok(dest)
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
