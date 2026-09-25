//! Store
//!
//! Write-backed capability roots for one run.

#![deny(missing_docs)]

use std::path::PathBuf;
use std::sync::Arc;

pub mod archive;
pub mod blob;
pub mod bundle;
pub mod fetch;
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
/// All three bases travel together from one construction site.
/// Empty paths read as unset.
#[derive(Debug, Clone, Default)]
pub struct StoreRoots {
    /// Config base holding slots and history.
    pub config_base: PathBuf,
    /// Cache base holding downloads and blobs.
    pub cache_base: PathBuf,
    /// Temp base holding spills and staging folders.
    pub temp_base: PathBuf,
}

/// Stateful write capabilities for one run.
///
/// One value covers every capability, built once per run.
/// Roots stay explicit at construction. Backends run on
/// the host or over the memory driver under a test guard:
/// the choice rides construction surroundings, never a
/// second constructor.
#[derive(Clone)]
pub struct Stores {
    /// Base folders for every write-backed capability.
    pub roots: StoreRoots,
    /// Trusted project files behind exec-rooted handles.
    resources: Arc<Resources>,
    /// Content-addressed blob pool.
    blobs: Arc<BlobStore>,
    /// Content-addressed fetch cache.
    fetch: Arc<FetchCache>,
    /// Compressed archive member listing and extraction.
    archives: Arc<ArchiveStore>,
    /// Portable bundle archive reads and writes.
    bundles: Arc<BundleStore>,
    /// Applied state slot with named slots and history.
    slots: Arc<SlotStore>,
}

impl StoreRoots {
    /// Standard host roots from environment folders.
    ///
    /// Config rides the `dirs` config folder under
    /// `confit`, cache rides the `dirs` cache folder
    /// under `confit`, temp rides the process temp
    /// folder. Unset folders fall back to relative
    /// `confit` folders.
    pub fn standard() -> Self {
        let config_base = dirs::config_dir()
            .map(|base| base.join("confit"))
            .unwrap_or_else(|| PathBuf::from("confit"));
        let cache_base = dirs::cache_dir()
            .map(|base| base.join("confit"))
            .unwrap_or_else(|| PathBuf::from("confit"));
        let temp_base = std::env::temp_dir();
        Self {
            config_base,
            cache_base,
            temp_base,
        }
    }
}

impl Stores {
    /// Stores over file backends.
    ///
    /// Roots arrive explicit from construction. One
    /// constructor serves runs and tests alike: under a
    /// test guard the driver holds memory, elsewhere the
    /// host. Downloads ride the network in production and
    /// the script registry under test; tests script bodies
    /// through the transport registry functions.
    pub fn new(roots: StoreRoots) -> Self {
        Self {
            resources: Arc::new(Resources::new(&roots)),
            blobs: Arc::new(BlobStore::new(&roots)),
            fetch: Arc::new(FetchCache::new(&roots)),
            archives: Arc::new(ArchiveStore::new(&roots)),
            bundles: Arc::new(BundleStore::new(&roots)),
            slots: Arc::new(SlotStore::new(&roots)),
            roots,
        }
    }

    /// Reads trusted project files behind exec-rooted handles.
    pub fn resources(&self) -> Arc<Resources> {
        self.resources.clone()
    }

    /// Reads the content-addressed blob pool.
    pub fn blobs(&self) -> Arc<BlobStore> {
        self.blobs.clone()
    }

    /// Reads the content-addressed fetch cache.
    pub fn fetch(&self) -> Arc<FetchCache> {
        self.fetch.clone()
    }

    /// Reads the compressed archive store.
    pub fn archives(&self) -> Arc<ArchiveStore> {
        self.archives.clone()
    }

    /// Reads the portable bundle store.
    pub fn bundles(&self) -> Arc<BundleStore> {
        self.bundles.clone()
    }

    /// Reads the applied state slot store.
    pub fn slots(&self) -> Arc<SlotStore> {
        self.slots.clone()
    }
}
