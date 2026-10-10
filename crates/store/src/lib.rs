//! Store
//! Write-backed capability roots for one run.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::sync::Arc;

use confit_model::progress::ProgressSender;

pub mod archive;
pub mod blob;
pub mod bundle;
pub mod faults;
pub mod fetch;
pub mod handles;
pub mod resources;
pub mod slot;

use archive::ArchiveStore;
use blob::BlobStore;
use bundle::BundleStore;
use fetch::FetchCache;
use resources::Resources;
use slot::SlotStore;

/// Base folders for every write-backed capability.
///
/// All three bases travel together from one construction site. Empty paths are
/// unset.
#[derive(Debug, Clone, Default)]
pub struct StoreRoots {
    /// Slot and history storage base.
    pub config_base: PathBuf,
    /// Fetch cache and staging area for transient files.
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
    /// Content-addressed blob pool.
    blobs: Arc<BlobStore>,
    /// Content-addressed fetch cache.
    fetch: Arc<FetchCache>,
    /// Compressed archive member listing and extraction.
    archives: Arc<ArchiveStore>,
    /// Portable bundle archives.
    bundles: Arc<BundleStore>,
    /// Applied state slot with named slots and history.
    slots: Arc<SlotStore>,
}

impl StoreRoots {
    /// Build standard host roots from environment folders.
    ///
    /// - Config uses the `dirs` config folder under `confit`, temp uses the
    ///   process temp folder under `confit`, and the project root is the
    ///   working folder.
    /// - Unset folders fall back to relative `confit` folders, an unavailable
    ///   working folder falls back to `.`.
    pub fn standard() -> Self {
        let config_base = dirs::config_dir()
            .map(|base| base.join("confit"))
            .unwrap_or_else(|| PathBuf::from("confit"));
        let temp_base = std::env::temp_dir().join("confit");
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            config_base,
            temp_base,
            project_root,
        }
    }
}

impl Stores {
    /// Build stores over file backends.
    pub fn new(roots: StoreRoots, progress: ProgressSender) -> Self {
        let resources = Arc::new(Resources::new(&roots));
        let blobs = Arc::new(BlobStore::new(&roots, progress.clone()));
        let archives = Arc::new(ArchiveStore::new(
            &roots,
            resources.clone(),
            progress.clone(),
        ));
        Self {
            resources: resources.clone(),
            blobs: blobs.clone(),
            fetch: Arc::new(FetchCache::new(&roots, progress)),
            archives: archives.clone(),
            bundles: Arc::new(BundleStore::new(archives, blobs, resources)),
            slots: Arc::new(SlotStore::new(&roots)),
            roots,
        }
    }

    /// Return trusted project files.
    pub fn resources(&self) -> Arc<Resources> {
        self.resources.clone()
    }

    /// Return the content-addressed blob pool.
    pub fn blobs(&self) -> Arc<BlobStore> {
        self.blobs.clone()
    }

    /// Return the content-addressed fetch cache.
    pub fn fetch(&self) -> Arc<FetchCache> {
        self.fetch.clone()
    }

    /// Return the compressed archive store.
    pub fn archives(&self) -> Arc<ArchiveStore> {
        self.archives.clone()
    }

    /// Return the portable bundle store.
    pub fn bundles(&self) -> Arc<BundleStore> {
        self.bundles.clone()
    }

    /// Return the applied state slot store.
    pub fn slots(&self) -> Arc<SlotStore> {
        self.slots.clone()
    }
}
