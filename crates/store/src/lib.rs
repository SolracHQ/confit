//! Trusted roots and content caches.

#![deny(missing_docs)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_core::error::ContextualResult;
use confit_driver::paths::BaseDirs;
use confit_events::EventHandles;

pub mod archive;
pub mod fetch;
pub mod pool;
pub mod resources;
pub mod state;

use archive::ArchiveStore;
use fetch::FetchCache;
use pool::PoolStore;
use resources::Resources;
use state::StateStore;

/// The base folders of one execution.
#[derive(Debug, Clone, Default)]
pub struct StoreRoots {
    /// The base folder for history and named states.
    pub config_base: PathBuf,
    /// The base folder for the fetch cache and transient files.
    pub temp_base: PathBuf,
    /// The folder that bounds every trusted project file.
    pub project_root: PathBuf,
}

/// The trusted roots and content caches.
#[derive(Clone)]
pub struct Stores {
    /// The owned base folders.
    pub roots: StoreRoots,
    /// The owned project files.
    resources: Arc<Resources>,
    /// The compressed blobs.
    pool: Arc<PoolStore>,
    /// The remote bytes by content hash.
    fetch: Arc<FetchCache>,
    /// The unpacked archive members.
    archives: Arc<ArchiveStore>,
    /// The named states and their history.
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

impl Stores {
    /// Builds every store over file backends.
    pub fn new(roots: StoreRoots, handles: EventHandles) -> Self {
        let pool = Arc::new(PoolStore::new(&roots, handles.clone()));
        let resources = Arc::new(Resources::new(&roots, pool.clone(), handles.clone()));
        let archives = Arc::new(ArchiveStore::new(resources.clone(), handles.clone()));
        Self {
            resources: resources.clone(),
            pool: pool.clone(),
            fetch: Arc::new(FetchCache::new(resources.clone(), handles.clone())),
            archives,
            states: Arc::new(StateStore::new(&roots, resources.clone(), handles)),
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

    /// Removes pool entries that no stored state references.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] when a folder is unreadable.
    pub fn prune_unreferenced(&self) -> ContextualResult<usize> {
        let keep = self.states.keep_set()?;
        self.pool.prune(&keep)
    }
}

/// Returns the working folder, or `.` on failure.
fn current_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}
