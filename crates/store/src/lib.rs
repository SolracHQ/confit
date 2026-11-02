//! Write-backed capability roots for one run.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_driver::paths::BaseDirs;
use confit_ui::ToUi;
use crossbeam_channel::Sender;

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

/// The base folders of one run.
#[derive(Debug, Clone, Default)]
pub struct StoreRoots {
    /// The base folder for history and named states.
    pub config_base: PathBuf,
    /// The base folder for the fetch cache and transient files.
    pub temp_base: PathBuf,
    /// The folder that bounds every trusted project file.
    pub project_root: PathBuf,
}

/// The write-backed stores for one run.
#[derive(Clone)]
pub struct Stores {
    /// The base folders for every store.
    pub roots: StoreRoots,
    /// The trusted project files shared by every store.
    resources: Arc<Resources>,
    /// The pool that holds content-addressed bytes.
    pool: Arc<PoolStore>,
    /// The cache that holds remote bytes by content hash.
    fetch: Arc<FetchCache>,
    /// The store that lists and extracts archive members.
    archives: Arc<ArchiveStore>,
    /// The store that holds named state files and their history.
    states: Arc<StateStore>,
}

impl StoreRoots {
    /// Builds host roots under the standard environment folders.
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

    /// Builds test roots under one temporary folder.
    pub fn under(temp: &Path, project_root: &Path) -> Self {
        Self {
            config_base: temp.join("confit"),
            temp_base: temp.join("confit-temp"),
            project_root: project_root.to_path_buf(),
        }
    }
}

/// Returns the working folder, or `.` on failure.
fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

impl Stores {
    /// Builds every store over file backends.
    pub fn new(roots: StoreRoots, to_ui: Sender<ToUi>) -> Self {
        let pool = Arc::new(PoolStore::new(&roots, to_ui.clone()));
        let resources = Arc::new(Resources::new(&roots, pool.clone()));
        let archives = Arc::new(ArchiveStore::new(resources.clone(), to_ui.clone()));
        Self {
            resources: resources.clone(),
            pool: pool.clone(),
            fetch: Arc::new(FetchCache::new(resources.clone(), to_ui)),
            archives,
            states: Arc::new(StateStore::new(&roots)),
            roots,
        }
    }

    /// Returns the store of trusted project files.
    pub fn resources(&self) -> Arc<Resources> {
        self.resources.clone()
    }

    /// Returns the pool of content-addressed bytes.
    pub fn pool(&self) -> Arc<PoolStore> {
        self.pool.clone()
    }

    /// Returns the cache of remote bytes.
    pub fn fetch(&self) -> Arc<FetchCache> {
        self.fetch.clone()
    }

    /// Returns the store of archive members.
    pub fn archives(&self) -> Arc<ArchiveStore> {
        self.archives.clone()
    }

    /// Returns the store of state files and history.
    pub fn states(&self) -> Arc<StateStore> {
        self.states.clone()
    }
}
