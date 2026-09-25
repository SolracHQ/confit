//! Apply
//!
//! Destination writes, removals, and drift.

#![deny(missing_docs)]

mod disk;
mod drift;
mod remove;
mod resolve;
mod snapshot;
mod write;

pub use disk::{Disk, DiskKind, HostDisk, MemoryDisk};
pub use snapshot::{Snapshot, TreeMemberSnapshot};

use confit_store::{StoreRoots, Stores};

/// Destination applier behind snapshots, writes, and drift.
///
/// Roots arrive explicit at construction. Host reads the live
/// filesystem, memory reads a seeded map. Blob bytes resolve
/// through the held stores alone.
pub struct Applier {
    /// Holds the write capabilities behind blob resolution.
    pub(crate) stores: Stores,
    /// Serves every disk read and write behind the verbs.
    pub(crate) disk: Box<dyn Disk>,
}

impl Applier {
    /// Applier for CLI wiring.
    ///
    /// Roots arrive explicit from CLI wiring. File backends
    /// serve every read and write.
    pub fn host(roots: StoreRoots) -> Self {
        Self::with_stores(Stores::host(roots), DiskKind::Host)
    }

    /// Applier for tests.
    ///
    /// Roots arrive explicit from test setup. Memory disk
    /// serves snapshots and writes.
    pub fn memory(roots: StoreRoots) -> Self {
        Self::with_stores(Stores::memory(roots), DiskKind::Memory)
    }

    /// Applier over shared stores.
    ///
    /// The disk backend arrives explicit beside the stores,
    /// so callers sharing one `Stores` keep blobs and disk
    /// behind one value.
    pub fn with_stores(stores: Stores, disk: DiskKind) -> Self {
        match disk {
            DiskKind::Host => Self {
                stores,
                disk: Box::new(HostDisk),
            },
            DiskKind::Memory => Self {
                stores,
                disk: Box::new(MemoryDisk::new()),
            },
        }
    }

    /// Reads the write capabilities behind blob resolution.
    pub fn stores(&self) -> &Stores {
        &self.stores
    }
}
