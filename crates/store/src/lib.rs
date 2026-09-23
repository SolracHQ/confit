//! Store
//!
//! Write-backed capability roots for one run.

#![deny(missing_docs)]

use std::path::PathBuf;

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
/// Roots stay explicit at construction.
#[derive(Debug, Clone, Default)]
pub struct Stores {
    /// Base folders for every write-backed capability.
    pub roots: StoreRoots,
}

impl Stores {
    /// Stores for CLI wiring.
    ///
    /// Roots arrive explicit from CLI wiring.
    pub fn host(roots: StoreRoots) -> Self {
        Self { roots }
    }

    /// Stores for tests.
    ///
    /// Roots arrive explicit from test setup.
    pub fn memory(roots: StoreRoots) -> Self {
        Self { roots }
    }
}
