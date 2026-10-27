//! Store write-backed capability roots for one run.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_core::progress::ProgressSender;
use confit_driver::paths::BaseDirs;

pub mod archive;
pub mod fetch;
pub mod handles;
pub mod pool;
pub mod resources;
pub mod state;

use archive::ArchiveStore;
use fetch::FetchCache;
use pool::PoolStore;
use resources::Resources;
use state::StateStore;

/// Base folders for every write-backed capability.
#[derive(Debug, Clone, Default)]
pub struct StoreRoots {
    /// State and history storage base.
    pub config_base: PathBuf,
    /// Fetch cache and pending area for transient files.
    pub temp_base: PathBuf,
    /// Project boundary for trusted files.
    pub project_root: PathBuf,
}

/// Stateful write capabilities for one run.
#[derive(Clone)]
pub struct Stores {
    /// Base folders for every write-backed capability.
    pub roots: StoreRoots,
    /// Trusted project files.
    resources: Arc<Resources>,
    /// Content-addressed pool.
    pool: Arc<PoolStore>,
    /// Content-addressed fetch cache.
    fetch: Arc<FetchCache>,
    /// Compressed archive member listing and extraction.
    archives: Arc<ArchiveStore>,
    /// Named state files with history.
    states: Arc<StateStore>,
}

impl StoreRoots {
    /// Build standard host roots from environment folders.
    pub fn standard() -> Self {
        let config_base = BaseDirs::new()
            .config
            .map(|base| base.join("confit"))
            .unwrap_or_else(|| PathBuf::from("confit"));
        Self {
            config_base,
            temp_base: std::env::temp_dir().join("confit"),
            project_root: current_dir(),
        }
    }

    /// Build roots under one temp folder for tests.
    pub fn under(temp: &Path, project_root: &Path) -> Self {
        Self {
            config_base: temp.join("confit"),
            temp_base: temp.join("confit-temp"),
            project_root: project_root.to_path_buf(),
        }
    }
}

/// Read the working folder with a relative fallback.
fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl Stores {
    /// Build stores over file backends.
    pub fn new(roots: StoreRoots, progress: ProgressSender) -> Self {
        let pool = Arc::new(PoolStore::new(&roots, progress.clone()));
        let resources = Arc::new(Resources::new(&roots, pool.clone()));
        let archives = Arc::new(ArchiveStore::new(resources.clone(), progress.clone()));
        Self {
            resources: resources.clone(),
            pool: pool.clone(),
            fetch: Arc::new(FetchCache::new(resources.clone(), progress)),
            archives,
            states: Arc::new(StateStore::new(&roots)),
            roots,
        }
    }

    /// Return trusted project files.
    pub fn resources(&self) -> Arc<Resources> {
        self.resources.clone()
    }

    /// Return the content-addressed pool.
    pub fn pool(&self) -> Arc<PoolStore> {
        self.pool.clone()
    }

    /// Return the content-addressed fetch cache.
    pub fn fetch(&self) -> Arc<FetchCache> {
        self.fetch.clone()
    }

    /// Return the compressed archive store.
    pub fn archives(&self) -> Arc<ArchiveStore> {
        self.archives.clone()
    }

    /// Return named state files with history.
    pub fn states(&self) -> Arc<StateStore> {
        self.states.clone()
    }
}
