//! Resolves the base folders for route expansion.

use std::path::PathBuf;

/// The host base folders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseDirs {
    /// The home folder of the current user.
    pub home: Option<PathBuf>,
    /// The config folder of the platform.
    pub config: Option<PathBuf>,
    /// The data folder of the platform.
    pub data: Option<PathBuf>,
    /// The cache folder of the platform.
    pub cache: Option<PathBuf>,
}

impl Default for BaseDirs {
    /// Builds the default base folders.
    fn default() -> Self {
        Self::new()
    }
}

impl BaseDirs {
    /// Builds the base folders from the host.
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
    //! Holds the temp root for the calling thread.

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
                    Err(error) => panic!("fails to create the test temp root: {error}"),
                }
            }
            match slot.as_ref() {
                Some(dir) => dir.path().to_path_buf(),
                None => panic!("expected a test temp root"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_dirs_share_one_temp_root_across_calls() {
        let first = BaseDirs::new();
        let second = BaseDirs::new();
        assert_eq!(first, second, "both calls share one root");
        let home = first.home.unwrap();
        let parent = home.parent().unwrap().to_path_buf();
        assert!(parent.is_dir(), "the shared root exists on disk");
        for root in [&first.config, &first.data, &first.cache] {
            assert_eq!(
                root.as_ref().unwrap().parent().unwrap(),
                parent,
                "every base folder shares the root"
            );
        }
    }
}
