//! Resources
//!
//! Trusted-source birth and text reads.

pub mod error;

use std::path::{Component, Path};

use confit_model::sha::Sha;

use crate::StoreRoots;
use crate::handles::{ResourceHandle, TrustedHandle};
use confit_driver as driver;
use error::{ResourceError, Result};

/// Fallback mode for spill entries without distinct bits.
const DEFAULT_SPILL_MODE: u32 = 0o644;

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
    /// - [`ResourceError::Escape`] for escaping files.
    /// - [`ResourceError::Missing`] for missing files.
    /// - [`ResourceError::Denied`] for denied files.
    /// - [`ResourceError::Unknown`] for other failures.
    pub fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle> {
        if !path.starts_with(exec_root) {
            return Err(ResourceError::Escape {
                path: path.to_path_buf(),
            });
        }
        let mut file = match driver::fs::open(path) {
            Ok(file) => file,
            Err(error) => {
                return Err(ResourceError::from_io(path, error));
            }
        };
        let sha = Sha::read(&mut file).map_err(|failure| ResourceError::from_io(path, failure))?;
        ResourceHandle::new(exec_root, path.to_path_buf(), sha).map_err(|_| ResourceError::Escape {
            path: path.to_path_buf(),
        })
    }

    /// Reads one trusted file as text.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Missing`] for missing files.
    /// - [`ResourceError::Denied`] for denied files.
    /// - [`ResourceError::Unknown`] for other failures.
    pub fn read_text(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(handle.canonical())
    }

    /// Births a spill handle for one staged file.
    ///
    /// Birth checks containment under the temp base once,
    /// downstream code trusts the handle type without rechecking.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Escape`] for escaping files.
    pub fn cache(&self, dir: &Path, name: &str, sha: Sha) -> Result<ResourceHandle> {
        if name.is_empty() {
            return Err(ResourceError::Escape {
                path: dir.join(name),
            });
        }
        for part in Path::new(name).components() {
            match part {
                Component::RootDir | Component::Prefix(_) | Component::ParentDir => {
                    return Err(ResourceError::Escape {
                        path: dir.join(name),
                    });
                }
                Component::CurDir | Component::Normal(_) => {}
            }
        }
        let joined = dir.join(name);
        if !joined.starts_with(&self.roots.temp_base) {
            return Err(ResourceError::Escape { path: joined });
        }
        ResourceHandle::new(dir, joined.clone(), sha)
            .map_err(|_| ResourceError::Escape { path: joined })
    }

    /// Streams one handle for reading.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Missing`] for missing files.
    /// - [`ResourceError::Denied`] for denied files.
    /// - [`ResourceError::Unknown`] for other failures.
    pub fn open(&self, handle: &ResourceHandle) -> Result<Box<dyn std::io::Read>> {
        let path = handle.canonical();
        driver::fs::open(path)
            .map(|file| file as Box<dyn std::io::Read>)
            .map_err(|error| ResourceError::from_io(path, error))
    }

    /// Reads permission bits for one handle.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Missing`] for missing files.
    /// - [`ResourceError::Denied`] for denied files.
    /// - [`ResourceError::Unknown`] for other failures.
    pub fn mode(&self, handle: &ResourceHandle) -> Result<u32> {
        let path = handle.canonical();
        if driver::fs::read_link(path).is_ok() {
            return Ok(DEFAULT_SPILL_MODE);
        }
        driver::fs::mode(path).map_err(|error| ResourceError::from_io(path, error))
    }
}

/// Reads one file as text.
///
/// # Errors
///
/// - [`ResourceError::Unknown`] for invalid text and other failures.
/// - [`ResourceError::Missing`] for missing files.
/// - [`ResourceError::Denied`] for denied files.
fn read_text(path: &Path) -> Result<String> {
    match driver::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| ResourceError::Unknown {
            path: path.to_path_buf(),
            message: error.to_string(),
        }),
        Err(error) => Err(ResourceError::from_io(path, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;

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
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&path, b"return {}").unwrap();
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
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&outside, b"return {}").unwrap();
        match store.resource(&root, &outside) {
            Ok(_) => panic!("escaping resource passes"),
            Err(ResourceError::Escape { path }) => {
                assert_eq!(path, outside, "escape names the path")
            }
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let nested = root.join("sub").join("note.lua");
        driver::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        driver::fs::write(&nested, b"hi").unwrap();
        let other_root = dir.path().join("other");
        match store.resource(&other_root, &nested) {
            Ok(_) => panic!("foreign root passes"),
            Err(ResourceError::Escape { .. }) => {}
            Err(error) => panic!("wrong foreign variant: {error}"),
        }
    }

    #[test]
    fn resource_names_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let store = test_store();
        let root = dir.path().join("project");
        let missing = root.join("absent.lua");
        driver::fs::create_dir_all(&root).unwrap();
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
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&path, &[0xFF, 0xFE, b'x']).unwrap();
        match store.resource(&root, &path) {
            Ok(handle) => match store.read_text(&handle) {
                Ok(_) => panic!("invalid text passes"),
                Err(ResourceError::Unknown { path: found, .. }) => {
                    assert_eq!(found, path, "binary reports the path")
                }
                Err(error) => panic!("wrong text variant: {error}"),
            },
            Err(error) => panic!("binary resource births: {error}"),
        }
    }

    #[test]
    fn cache_refuses_outside_temp() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = StoreRoots {
            temp_base: dir.path().join("temp"),
            ..Default::default()
        };
        let store = Resources::new(&roots);
        let spill = roots.temp_base.join("extract").join("abc");
        driver::fs::create_dir_all(&spill).unwrap();
        let path = spill.join("a.txt");
        driver::fs::write(&path, b"alpha").unwrap();
        let handle = match store.cache(&spill, "a.txt", Sha::hash(b"alpha")) {
            Ok(handle) => handle,
            Err(error) => panic!("spill births: {error}"),
        };
        assert_eq!(handle.canonical(), path.as_path());
        let mut found = Vec::new();
        match store.open(&handle) {
            Ok(mut reader) => {
                reader.read_to_end(&mut found).unwrap();
            }
            Err(error) => panic!("spill opens: {error}"),
        }
        assert_eq!(found, b"alpha");
        match store.mode(&handle) {
            Ok(_) => {}
            Err(error) => panic!("spill modes: {error}"),
        }
        for evil in ["../evil.txt", "/evil.txt", ""] {
            match store.cache(&spill, evil, Sha::hash(b"x")) {
                Ok(_) => panic!("escaping spill passes"),
                Err(ResourceError::Escape { .. }) => {}
                Err(error) => panic!("wrong spill variant: {error}"),
            }
        }
        let outside = dir.path().join("elsewhere");
        driver::fs::create_dir_all(&outside).unwrap();
        match store.cache(&outside, "a.txt", Sha::hash(b"x")) {
            Ok(_) => panic!("outside spill passes"),
            Err(ResourceError::Escape { .. }) => {}
            Err(error) => panic!("wrong outside variant: {error}"),
        }
    }

    #[test]
    fn open_names_missing_spill() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let roots = StoreRoots {
            temp_base: dir.path().join("temp"),
            ..Default::default()
        };
        let store = Resources::new(&roots);
        let spill = roots.temp_base.join("extract").join("abc");
        driver::fs::create_dir_all(&spill).unwrap();
        let handle = match store.cache(&spill, "absent.txt", Sha::hash(b"x")) {
            Ok(handle) => handle,
            Err(error) => panic!("absent births: {error}"),
        };
        match store.open(&handle) {
            Ok(_) => panic!("absent spill passes"),
            Err(ResourceError::Missing { path }) => {
                assert!(path.ends_with("absent.txt"), "absent names file")
            }
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }
}
