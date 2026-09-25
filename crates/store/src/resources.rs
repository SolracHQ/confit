//! Resources
//!
//! Trusted-source birth and text reads.

use std::path::Path;

use confit_model::error::{Error, Result};
use confit_model::handles::{ResourceHandle, Sha, TrustedHandle};

use crate::StoreRoots;
use confit_driver as driver;

/// Trusted project files behind exec-rooted handles.
///
/// Birth checks containment under the exec root once;
/// downstream code trusts the handle type without rechecking.
#[derive(Debug, Clone)]
pub struct Resources {
    /// Holds the store roots backing construction.
    pub roots: StoreRoots,
}

impl Resources {
    /// Builds file-backed resources under explicit roots.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            roots: roots.clone(),
        }
    }

    /// Births a resource handle for one project file.
    ///
    /// # Errors
    ///
    /// Escaping, missing, and unreadable files fail as plan or io errors.
    pub fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle> {
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        let mut file = match driver::open_read(path) {
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

    /// Reads one trusted file as text.
    ///
    /// # Errors
    ///
    /// Missing files and invalid text fail as plan errors.
    pub fn read_text(&self, handle: &ResourceHandle) -> Result<String> {
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
    match driver::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| Error::Plan(format!("cannot read '{}': invalid text", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::Plan(format!(
            "cannot read '{}': missing file",
            path.display()
        ))),
        Err(error) => Err(Error::from(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::TestGuard;

    fn test_store() -> Resources {
        Resources::new(&StoreRoots::default())
    }

    #[test]
    fn resource_births_handle_and_reads_text() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store();
        let root = dir.path().join("project");
        let path = root.join("profile.lua");
        driver::create_dir_all(&root).unwrap();
        driver::write(&path, b"return {}").unwrap();
        match store.resource(&root, &path) {
            Ok(handle) => {
                assert_eq!(handle.canonical(), path.as_path());
                assert_eq!(handle.sha(), &Sha::hash(b"return {}"));
                match store.read_text(&handle) {
                    Ok(found) => assert_eq!(found, "return {}"),
                    Err(error) => panic!("trusted text reads: {error}"),
                }
            }
            Err(error) => panic!("resource births: {error}"),
        }
    }

    #[test]
    fn resource_refuses_escape_plus_exec_root_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store();
        let root = dir.path().join("project");
        let outside = dir.path().join("outside.lua");
        driver::create_dir_all(&root).unwrap();
        driver::write(&outside, b"return {}").unwrap();
        match store.resource(&root, &outside) {
            Ok(_) => panic!("escaping resource passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("escapes"), "escape reports itself: {text}");
                assert!(
                    text.contains(&outside.display().to_string()),
                    "escape names the path: {text}"
                );
                assert!(
                    text.contains(&root.display().to_string()),
                    "escape names the root: {text}"
                );
            }
        }
        let nested = root.join("sub").join("note.lua");
        driver::create_dir_all(nested.parent().unwrap()).unwrap();
        driver::write(&nested, b"hi").unwrap();
        let other_root = dir.path().join("other");
        match store.resource(&other_root, &nested) {
            Ok(_) => panic!("foreign root passes"),
            Err(error) => assert!(
                error.to_string().contains("escapes"),
                "foreign root reports escape: {error}"
            ),
        }
    }

    #[test]
    fn resource_names_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store();
        let root = dir.path().join("project");
        let missing = root.join("absent.lua");
        driver::create_dir_all(&root).unwrap();
        match store.resource(&root, &missing) {
            Ok(_) => panic!("absent resource passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains(&missing.display().to_string()),
                    "absent names the path: {text}"
                );
                assert!(text.contains("missing file"), "absent reports loss: {text}");
            }
        }
    }

    #[test]
    fn read_text_refuses_invalid_text() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store();
        let root = dir.path().join("project");
        let path = root.join("binary.lua");
        driver::create_dir_all(&root).unwrap();
        driver::write(&path, &[0xFF, 0xFE, b'x']).unwrap();
        match store.resource(&root, &path) {
            Ok(handle) => match store.read_text(&handle) {
                Ok(_) => panic!("invalid text passes"),
                Err(error) => assert!(
                    error.to_string().contains("invalid text"),
                    "binary reports text loss: {error}"
                ),
            },
            Err(error) => panic!("binary resource births: {error}"),
        }
    }
}
