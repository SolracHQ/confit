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

use archive::{ArchiveStore, file::FileArchiveStore, memory::MemoryArchiveStore};
use blob::{BlobStore, file::FileBlobStore, memory::MemoryBlobStore};
use bundle::{BundleStore, file::FileBundleStore, memory::MemoryBundleStore};
use fetch::{FetchCache, file::FileFetchCache, memory::MemoryFetchCache};
use resources::{Resources, file::FileResources, memory::MemoryResources};
use slot::{SlotStore, file::FileSlotStore, memory::MemorySlotStore};

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
/// Roots stay explicit at construction. Host builds file
/// backends, memory builds fakes.
#[derive(Clone)]
pub struct Stores {
    /// Base folders for every write-backed capability.
    pub roots: StoreRoots,
    /// Trusted project files behind exec-rooted handles.
    resources: Arc<dyn Resources>,
    /// Content-addressed blob pool.
    blobs: Arc<dyn BlobStore>,
    /// Content-addressed fetch cache.
    fetch: Arc<dyn FetchCache>,
    /// Compressed archive member listing and extraction.
    archives: Arc<dyn ArchiveStore>,
    /// Portable bundle archive reads and writes.
    bundles: Arc<dyn BundleStore>,
    /// Applied state slot with named slots and history.
    slots: Arc<dyn SlotStore>,
}

impl StoreRoots {
    /// Standard host roots from environment folders.
    ///
    /// Config rides `XDG_CONFIG_HOME` else home `.config`
    /// under `confit`. Cache rides `XDG_CACHE_HOME` else
    /// home `.cache` under `confit`. Temp rides the process
    /// temp folder. Unset homes fall back to relative
    /// `confit` folders.
    pub fn standard() -> Self {
        let home = std::env::var_os("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from);
        let config_base = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.clone().map(|home| home.join(".config")))
            .map(|base| base.join("confit"))
            .unwrap_or_else(|| PathBuf::from("confit"));
        let cache_base = std::env::var_os("XDG_CACHE_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| home.clone().map(|home| home.join(".cache")))
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
    /// Stores for CLI wiring.
    ///
    /// Roots arrive explicit from CLI wiring. File backends
    /// serve every capability.
    pub fn host(roots: StoreRoots) -> Self {
        Self {
            resources: Arc::new(FileResources::new(&roots)),
            blobs: Arc::new(FileBlobStore::new(&roots)),
            fetch: Arc::new(FileFetchCache::new(&roots)),
            archives: Arc::new(FileArchiveStore::new(&roots)),
            bundles: Arc::new(FileBundleStore::new(&roots)),
            slots: Arc::new(FileSlotStore::new(&roots)),
            roots,
        }
    }

    /// Stores for tests.
    ///
    /// Roots arrive explicit from test setup. Memory fakes
    /// serve every capability.
    pub fn memory(roots: StoreRoots) -> Self {
        Self {
            resources: Arc::new(MemoryResources::new()),
            blobs: Arc::new(MemoryBlobStore::new()),
            fetch: Arc::new(MemoryFetchCache::new(roots.cache_base.clone())),
            archives: Arc::new(MemoryArchiveStore::new()),
            bundles: Arc::new(MemoryBundleStore::new()),
            slots: Arc::new(MemorySlotStore::new()),
            roots,
        }
    }

    /// Stores from explicit backends.
    ///
    /// Roots arrive explicit beside one backend per
    /// capability. Tests seed fakes before assembling.
    pub fn assemble(
        roots: StoreRoots,
        resources: Arc<dyn Resources>,
        blobs: Arc<dyn BlobStore>,
        fetch: Arc<dyn FetchCache>,
        archives: Arc<dyn ArchiveStore>,
        bundles: Arc<dyn BundleStore>,
        slots: Arc<dyn SlotStore>,
    ) -> Self {
        Self {
            roots,
            resources,
            blobs,
            fetch,
            archives,
            bundles,
            slots,
        }
    }

    /// Reads trusted project files behind exec-rooted handles.
    pub fn resources(&self) -> Arc<dyn Resources> {
        self.resources.clone()
    }

    /// Reads the content-addressed blob pool.
    pub fn blobs(&self) -> Arc<dyn BlobStore> {
        self.blobs.clone()
    }

    /// Reads the content-addressed fetch cache.
    pub fn fetch(&self) -> Arc<dyn FetchCache> {
        self.fetch.clone()
    }

    /// Reads the compressed archive store.
    pub fn archives(&self) -> Arc<dyn ArchiveStore> {
        self.archives.clone()
    }

    /// Reads the portable bundle store.
    pub fn bundles(&self) -> Arc<dyn BundleStore> {
        self.bundles.clone()
    }

    /// Reads the applied state slot store.
    pub fn slots(&self) -> Arc<dyn SlotStore> {
        self.slots.clone()
    }
}
