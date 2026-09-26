//! Runtime
//!
//! Destination writes, removals, drift, and checks.

#![deny(missing_docs)]

pub mod checks;
mod disk;
mod drift;
mod remove;
mod render;
mod resolve;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, find_executable};

pub use disk::{HostDisk, Live, LiveMember};

use confit_model::progress::ProgressSender;
use confit_store::{StoreRoots, Stores};

/// Destination applier behind live reads, writes, and drift.
///
/// Roots arrive explicit at construction.
pub struct Applier {
    /// Write capabilities behind blob resolution.
    pub(crate) stores: Stores,
    /// Disk reads and writes.
    pub(crate) disk: HostDisk,
    /// Write events, `None` for silence.
    pub(crate) progress: Option<ProgressSender>,
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
            progress: None,
        }
    }

    /// Builder carrying the progress sender behind write events.
    ///
    /// `None` holds silence.
    pub fn with_progress(mut self, progress: Option<ProgressSender>) -> Self {
        self.progress = progress;
        self
    }

    /// Reads the write capabilities behind blob resolution.
    pub fn stores(&self) -> &Stores {
        &self.stores
    }
}
