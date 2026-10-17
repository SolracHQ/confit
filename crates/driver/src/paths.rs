//! Base folders.

use std::path::PathBuf;

/// Base folders for route expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseDirs {
    /// User home folder.
    pub home: Option<PathBuf>,
    /// Platform config folder.
    pub config: Option<PathBuf>,
    /// Platform data folder.
    pub data: Option<PathBuf>,
    /// Platform cache folder.
    pub cache: Option<PathBuf>,
}

impl Default for BaseDirs {
    /// Build base folders.
    fn default() -> Self {
        Self::new()
    }
}

impl BaseDirs {
    /// Read base folders.
    ///
    /// - Test builds share the calling thread temp root.
    ///
    /// # Panics
    ///
    /// - Temp failure panics on test builds.
    pub fn new() -> Self {
        #[cfg(any(test, feature = "test-support"))]
        {
            support::under(&support::test_root())
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            Self {
                home: dirs::home_dir(),
                config: dirs::config_dir(),
                data: dirs::data_dir(),
                cache: dirs::cache_dir(),
            }
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
mod support {
    //! Calling thread temp root.

    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    use super::BaseDirs;

    thread_local! {
        static TEST_DIR: RefCell<Option<tempfile::TempDir>> = const { RefCell::new(None) };
    }

    pub(super) fn under(root: &Path) -> BaseDirs {
        BaseDirs {
            home: Some(root.join("home")),
            config: Some(root.join("config")),
            data: Some(root.join("data")),
            cache: Some(root.join("cache")),
        }
    }

    pub(super) fn test_root() -> PathBuf {
        TEST_DIR.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                match tempfile::TempDir::new() {
                    Ok(dir) => *slot = Some(dir),
                    Err(error) => panic!("test temp root: {error}"),
                }
            }
            match slot.as_ref() {
                Some(dir) => dir.path().to_path_buf(),
                None => panic!("test temp root missing"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_shares_one_temp_root_across_calls() {
        let first = BaseDirs::new();
        let second = BaseDirs::new();
        assert_eq!(first, second, "repeated calls share one root");
        let home = first.home.unwrap();
        let parent = home.parent().unwrap().to_path_buf();
        assert!(parent.is_dir(), "the shared root lives on disk");
        for root in [&first.config, &first.data, &first.cache] {
            assert_eq!(
                root.as_ref().unwrap().parent().unwrap(),
                parent,
                "every base shares the root"
            );
        }
    }
}
