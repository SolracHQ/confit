//! Memory
//!
//! Memory resources for tests.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::handles::{ResourceHandle, Sha, TrustedHandle};

use super::Resources;

/// Memory resources for tests.
///
/// Files ride an in-memory map keyed by canonical path.
#[derive(Debug, Default)]
pub struct MemoryResources {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

impl MemoryResources {
    /// Builds empty memory resources.
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    /// Seeds one file behind a canonical path.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(path.to_path_buf(), bytes.to_vec());
    }
}

impl Resources for MemoryResources {
    fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle> {
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        let guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(path) {
            Some(bytes) => ResourceHandle::new(exec_root, path.to_path_buf(), Sha::hash(bytes)),
            None => Err(Error::Plan(format!(
                "cannot read '{}': missing file",
                path.display()
            ))),
        }
    }

    fn read_text(&self, handle: &ResourceHandle) -> Result<String> {
        let guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(handle.canonical()) {
            Some(bytes) => String::from_utf8(bytes.clone()).map_err(|_| {
                Error::Plan(format!(
                    "cannot read '{}': invalid text",
                    handle.canonical().display()
                ))
            }),
            None => Err(Error::Plan(format!(
                "cannot read '{}': missing file",
                handle.canonical().display()
            ))),
        }
    }
}
