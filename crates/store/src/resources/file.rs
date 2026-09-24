//! File
//!
//! File-backed resources.

use std::path::Path;

use confit_core::error::{Error, Result};
use confit_core::handles::{ResourceHandle, Sha, TrustedHandle};

use super::Resources;
use crate::StoreRoots;

/// File-backed resources behind trusted-source birth and text reads.
#[derive(Debug, Clone)]
pub struct FileResources {
    /// Holds the store roots backing construction.
    pub roots: StoreRoots,
}

impl FileResources {
    /// Builds file-backed resources under explicit roots.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            roots: roots.clone(),
        }
    }
}

impl Resources for FileResources {
    fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle> {
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Plan(format!(
                    "cannot read '{}': missing file",
                    path.display()
                )));
            }
            Err(error) => return Err(Error::from(error)),
        };
        let sha = Sha::read(&mut file)?;
        ResourceHandle::new(exec_root, path.to_path_buf(), sha)
    }

    fn read_text(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(handle.canonical())
    }
}

/// Reads one file as text.
///
/// Missing files and invalid text fail as plan errors.
///
/// # Errors
///
/// Missing files fail as plan errors naming absence, invalid
/// text fails as plan errors, unreadable files surface as io
/// errors.
fn read_text(path: &Path) -> Result<String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| Error::Plan(format!("cannot read '{}': invalid text", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::Plan(format!(
            "cannot read '{}': missing file",
            path.display()
        ))),
        Err(error) => Err(Error::from(error)),
    }
}
