//! Resources
//! Trusted sources and text.

pub mod error;

use std::path::{Component, Path};

use confit_model::sha::Sha;

use crate::StoreRoots;
use crate::handles::ResourceHandle;
use confit_driver as driver;
use error::{ResourceError, Result};

/// Fallback mode for spill entries.
const DEFAULT_SPILL_MODE: u32 = 0o644;

/// Trusted project files.
#[derive(Debug, Clone)]
pub struct Resources {
    /// The store roots for construction.
    pub roots: StoreRoots,
}

impl Resources {
    /// Build file-backed resources under explicit roots.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            roots: roots.clone(),
        }
    }

    /// Birth a resource handle for one project file.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Escape`] for escaping files.
    /// - [`ResourceError::Read`] for failed file reads.
    pub fn resource(&self, path: &Path) -> Result<ResourceHandle> {
        let exec_root = &self.roots.project_root;
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
        ResourceHandle::new(exec_root, path.to_path_buf(), sha)
    }

    /// Read one trusted file as text.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Read`] for failed file reads.
    pub fn read_text(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(handle.canonical())
    }

    /// Birth a spill handle for one staged file.
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
    }

    /// Stream one handle for reading.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Read`] for failed file reads.
    pub fn open(&self, handle: &ResourceHandle) -> Result<Box<dyn driver::fs::FsFile>> {
        let path = handle.canonical();
        driver::fs::open(path).map_err(|error| ResourceError::from_io(path, error))
    }

    /// Read permission bits for one handle.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Read`] for failed file reads.
    pub fn mode(&self, handle: &ResourceHandle) -> Result<u32> {
        let path = handle.canonical();
        if driver::fs::is_link(path) {
            return Ok(DEFAULT_SPILL_MODE);
        }
        driver::fs::mode(path).map_err(|error| ResourceError::from_io(path, error))
    }
}

/// Read one file as text.
///
/// # Errors
///
/// - [`ResourceError::Read`] for failed file reads.
fn read_text(path: &Path) -> Result<String> {
    match driver::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|error| ResourceError::Read {
            path: path.to_path_buf(),
            fault: crate::faults::AccessFault::Unknown {
                message: error.to_string(),
            },
        }),
        Err(error) => Err(ResourceError::from_io(path, error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::faults::AccessFault;
    use confit_driver::fs::TestGuard;

    fn test_store(root: &Path) -> Resources {
        Resources::new(&StoreRoots {
            project_root: root.to_path_buf(),
            ..Default::default()
        })
    }

    #[test]
    fn resource_births_handle_and_reads_text() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let root = dir.path().join("project");
        let path = root.join("profile.lua");
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&path, b"return {}").unwrap();
        let store = test_store(&root);
        match store.resource(&path) {
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
        let root = dir.path().join("project");
        let outside = dir.path().join("outside.lua");
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&outside, b"return {}").unwrap();
        let store = test_store(&root);
        match store.resource(&outside) {
            Ok(_) => panic!("escaping resource passes"),
            Err(ResourceError::Escape { path }) => {
                assert_eq!(path, outside, "escape shows the path")
            }
            Err(error) => panic!("wrong escape variant: {error}"),
        }
        let nested = root.join("sub").join("note.lua");
        driver::fs::create_dir_all(nested.parent().unwrap()).unwrap();
        driver::fs::write(&nested, b"hi").unwrap();
        let foreign = test_store(&dir.path().join("other"));
        match foreign.resource(&nested) {
            Ok(_) => panic!("foreign root passes"),
            Err(ResourceError::Escape { .. }) => {}
            Err(error) => panic!("wrong foreign variant: {error}"),
        }
    }

    #[test]
    fn resource_names_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let root = dir.path().join("project");
        let store = test_store(&root);
        let missing = root.join("absent.lua");
        driver::fs::create_dir_all(&root).unwrap();
        match store.resource(&missing) {
            Ok(_) => panic!("absent resource passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains(&missing.display().to_string()),
                    "absent shows the path: {text}"
                );
                assert!(text.contains("missing file"), "absent reports loss: {text}");
            }
        }
    }

    #[test]
    fn read_text_refuses_invalid_text() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let root = dir.path().join("project");
        let store = test_store(&root);
        let path = root.join("binary.lua");
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&path, &[0xFF, 0xFE, b'x']).unwrap();
        match store.resource(&path) {
            Ok(handle) => match store.read_text(&handle) {
                Ok(_) => panic!("invalid text passes"),
                Err(ResourceError::Read { path: found, fault }) => {
                    assert_eq!(found, path, "binary reports the path");
                    assert!(
                        matches!(fault, AccessFault::Unknown { .. }),
                        "binary keeps its text: {fault}"
                    );
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
            Err(ResourceError::Read { path, fault }) => {
                assert!(path.ends_with("absent.txt"), "absent shows the file");
                assert!(
                    matches!(fault, AccessFault::Missing),
                    "absent keeps the fault: {fault}"
                );
            }
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    #[test]
    fn open_reports_read_denied() {
        let dir = tempfile::tempdir().unwrap();
        let guard = TestGuard::install();
        let root = dir.path().join("project");
        let store = test_store(&root);
        let path = root.join("profile.lua");
        driver::fs::create_dir_all(&root).unwrap();
        driver::fs::write(&path, b"return {}").unwrap();
        let handle = match store.resource(&path) {
            Ok(handle) => handle,
            Err(error) => panic!("resource births: {error}"),
        };
        guard.fail_reads(std::io::ErrorKind::PermissionDenied);
        match store.open(&handle) {
            Ok(_) => panic!("refused file passes"),
            Err(ResourceError::Read { path: found, fault }) => {
                assert_eq!(found, path, "open keeps the path");
                assert!(matches!(fault, AccessFault::Denied), "open keeps the fault");
            }
            Err(error) => panic!("wrong open variant: {error}"),
        }
    }
}
