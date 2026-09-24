//! Slot
//!
//! Applied state slot, named slots, and manifest history.

use std::path::PathBuf;

use confit_core::error::Result;
use confit_core::plan::Bundle;
use confit_core::progress::ProgressSender;
use confit_core::store::SlotKind;
use confit_core::store::manifest::HistoryEntry;

pub mod file;
pub mod memory;

/// Applied state slot with named slots and history.
///
/// History lists newest first with `%N` picks from one.
pub trait SlotStore {
    /// Loads the applied manifest, treating missing slots as empty.
    ///
    /// # Errors
    ///
    /// Unreadable present slots fail as plan or io errors.
    fn load(&self) -> Result<Bundle>;

    /// Writes the applied slot and archives history with rotation.
    ///
    /// # Errors
    ///
    /// Clock and write failures surface as plan or io errors.
    fn store(&self, bundle: &Bundle, progress: Option<&ProgressSender>) -> Result<PathBuf>;

    /// Resolves one picker to its live bundle and slot kind.
    ///
    /// Absent pickers read the applied slot. `@name` reads the
    /// named slot. `%N` reads history newest-first from one.
    ///
    /// # Errors
    ///
    /// Absent slots, malformed and out-of-range picks fail as
    /// plan errors.
    fn resolve(&self, picker: Option<&str>) -> Result<(Bundle, SlotKind)>;

    /// Lists stored manifests newest first with apply picks.
    ///
    /// # Errors
    ///
    /// Folder resolution failures surface as plan errors.
    fn list_history(&self) -> Result<Vec<HistoryEntry>>;

    /// Writes one named slot holding its bundle manifest.
    ///
    /// # Errors
    ///
    /// Bad names and write failures surface as plan or io errors.
    fn store_named(&self, name: &str, bundle: &Bundle) -> Result<()>;

    /// Drops one named slot.
    ///
    /// # Errors
    ///
    /// Absent names fail as plan errors.
    fn delete_named(&self, name: &str) -> Result<()>;

    /// Reports whether no applied slot reads present.
    fn is_first_run(&self) -> bool;
}
