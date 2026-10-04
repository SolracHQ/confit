//! Runtime
//! Destination writes, removals, drift, and checks.

#![deny(missing_docs)]

pub mod checks;
mod disk;
mod drift;
pub mod error;
mod remove;
mod render;
mod write;

pub use checks::{Checks, DEFAULT_HOOK_TIMEOUT_SECS, find_executable};

pub use disk::{HostDisk, Live, LiveMember};

use confit_model::progress::ProgressSender;
use confit_store::{StoreRoots, Stores};

/// Destination applier.
///
/// Roots arrive explicit at construction.
pub struct Applier {
    /// Write capabilities for one run.
    pub(crate) stores: Stores,
    /// Disk reads and writes.
    pub(crate) disk: HostDisk,
    /// Write events.
    pub(crate) progress: ProgressSender,
}

impl Applier {
    /// Applier for CLI wiring.
    pub fn host(roots: StoreRoots) -> Self {
        let (sender, _) = crossbeam_channel::unbounded();
        Self::with_stores(Stores::new(roots, sender.clone()), sender)
    }

    /// Applier over shared stores.
    pub fn with_stores(stores: Stores, progress: ProgressSender) -> Self {
        Self {
            stores,
            disk: HostDisk,
            progress,
        }
    }

    /// Reads the write capabilities.
    pub fn stores(&self) -> &Stores {
        &self.stores
    }
}
