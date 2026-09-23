//! Slot
//!
//! Applied state slot, named slots, and manifest history.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::plan::Bundle;
use confit_core::progress::ProgressSender;
use confit_core::store::manifest::HistoryEntry;
use confit_core::store::slots::SlotKind;

/// Stored manifests kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 5;

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

    /// Drops one named slot.
    ///
    /// # Errors
    ///
    /// Absent names fail as plan errors.
    fn delete_named(&self, name: &str) -> Result<()>;

    /// Reports whether no applied slot reads present.
    fn is_first_run(&self) -> bool;
}

/// Memory slot store for tests.
///
/// Applied, named, and history bundles ride in-memory maps.
#[derive(Debug, Default)]
pub struct MemorySlotStore {
    applied: Mutex<Option<Bundle>>,
    named: Mutex<HashMap<String, Bundle>>,
    history: Mutex<Vec<Bundle>>,
    archived: Mutex<u64>,
}

impl MemorySlotStore {
    /// Builds an empty memory slot store.
    pub fn new() -> Self {
        Self {
            applied: Mutex::new(None),
            named: Mutex::new(HashMap::new()),
            history: Mutex::new(Vec::new()),
            archived: Mutex::new(0),
        }
    }
}

impl SlotStore for MemorySlotStore {
    fn load(&self) -> Result<Bundle> {
        let guard = match self.applied.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(guard.clone().unwrap_or_else(Bundle::empty))
    }

    fn store(&self, bundle: &Bundle, progress: Option<&ProgressSender>) -> Result<PathBuf> {
        let _ = progress;
        let mut applied = match self.applied.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *applied = Some(bundle.clone());
        let mut history = match self.history.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        history.push(bundle.clone());
        while history.len() > HISTORY_KEPT {
            history.remove(0);
        }
        let mut archived = match self.archived.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *archived += 1;
        Ok(PathBuf::from(format!("history/{}.json", *archived)))
    }

    fn resolve(&self, picker: Option<&str>) -> Result<(Bundle, SlotKind)> {
        let Some(raw) = picker else {
            let guard = match self.applied.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            match guard.clone() {
                Some(bundle) => return Ok((bundle, SlotKind::Applied)),
                None => {
                    return Err(Error::Plan(
                        "the applied slot reads absent, apply first".to_string(),
                    ));
                }
            }
        };
        if let Some(name) = raw.strip_prefix('@') {
            let guard = match self.named.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            match guard.get(name) {
                Some(bundle) => return Ok((bundle.clone(), SlotKind::Named(name.to_string()))),
                None => return Err(Error::Plan(format!("'@{name}' reads absent"))),
            }
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = rest.parse().map_err(|_| {
                Error::Plan(format!(
                    "'{raw}' reads unsupported, want '%N' holding a number from 1"
                ))
            })?;
            let guard = match self.history.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let total = guard.len();
            if pick < 1 || pick > total {
                return Err(Error::Plan(format!(
                    "'{raw}' reads out of range, holding {total} stored manifests"
                )));
            }
            match guard.iter().rev().nth(pick - 1).cloned() {
                Some(bundle) => return Ok((bundle, SlotKind::History(pick))),
                None => {
                    return Err(Error::Plan(format!(
                        "'{raw}' reads out of range, holding {total} stored manifests"
                    )));
                }
            }
        }
        Err(Error::Plan(format!(
            "'{raw}' reads unsupported, want '%N', '@name', or nothing"
        )))
    }

    fn list_history(&self) -> Result<Vec<HistoryEntry>> {
        let guard = match self.history.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        Ok(guard
            .iter()
            .rev()
            .enumerate()
            .map(|(position, _)| HistoryEntry {
                index: position + 1,
            })
            .collect())
    }

    fn delete_named(&self, name: &str) -> Result<()> {
        let mut guard = match self.named.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.remove(name) {
            Some(_) => Ok(()),
            None => Err(Error::Plan(format!("'@{name}' reads absent"))),
        }
    }

    fn is_first_run(&self) -> bool {
        let guard = match self.applied.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.is_none()
    }
}
