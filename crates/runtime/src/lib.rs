//! Runtime
//!
//! Destination writes, removals, drift, and checks.

#![deny(missing_docs)]

pub mod checks;
mod disk;
mod drift;
mod remove;
pub mod render;
mod resolve;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, find_executable};

pub use disk::{HostDisk, Live, LiveMember};

use confit_store::{StoreRoots, Stores};

/// Destination applier behind live reads, writes, and drift.
///
/// Roots arrive explicit at construction. Disk reads ride
/// the driver: host paths in production, memory under a
/// test guard. Blob bytes resolve through the held stores
/// alone.
pub struct Applier {
    /// Holds the write capabilities behind blob resolution.
    pub(crate) stores: Stores,
    /// Serves every disk read and write behind the verbs.
    pub(crate) disk: HostDisk,
}

impl Applier {
    /// Applier for CLI wiring.
    ///
    /// Roots arrive explicit from CLI wiring. File backends
    /// serve every read and write.
    pub fn host(roots: StoreRoots) -> Self {
        Self::with_stores(Stores::new(roots))
    }

    /// Applier over shared stores.
    ///
    /// The stores arrive explicit, so callers sharing one
    /// `Stores` keep blobs and disk behind one value.
    pub fn with_stores(stores: Stores) -> Self {
        Self {
            stores,
            disk: HostDisk,
        }
    }

    /// Reads the write capabilities behind blob resolution.
    pub fn stores(&self) -> &Stores {
        &self.stores
    }
}
