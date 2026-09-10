//! In-memory [`StateStore`] and [`PlanWriter`] fakes for tests.
//!
//! The service layer takes its seams as trait objects or generics, so unit
//! tests substitute these fakes instead of touching the filesystem.

use std::cell::RefCell;
use std::path::Path;

use crate::error::Result;
use crate::model::Plan;

use super::traits::{PlanFormat, PlanWriter, State, StateStore};

/// In-memory [`StateStore`] returning a cloned snapshot.
///
/// Invariants: [`load`](StateStore::load) resolves from the in-memory
/// snapshot, always succeeding; each call returns a fresh clone.
#[derive(Debug, Clone, Default)]
pub struct MemoryState {
    state: State,
}

impl MemoryState {
    /// Empty state store.
    pub fn new() -> Self {
        Self {
            state: State::empty(),
        }
    }

    /// State store preloaded with `state`.
    ///
    /// Args: `state` is the snapshot every [`load`](StateStore::load) clones.
    pub fn with_state(state: State) -> Self {
        Self { state }
    }
}

impl StateStore for MemoryState {
    /// Clone the preloaded snapshot.
    fn load(&self) -> Result<State> {
        Ok(self.state.clone())
    }
}

/// In-memory [`PlanWriter`] capturing the last rendered plan.
///
/// Invariants: holds at most one capture; [`take`](MemoryWriter::take)
/// drains it, so each assertion consumes exactly one `write`.
#[derive(Debug, Default)]
pub struct MemoryWriter {
    captured: RefCell<Option<(String, PlanFormat)>>,
}

impl MemoryWriter {
    /// Empty writer awaiting its first capture.
    ///
    /// Example:
    /// ```rust
    /// use confit::store::MemoryWriter;
    ///
    /// let writer = MemoryWriter::new();
    /// assert!(writer.take().is_none());
    /// ```
    pub fn new() -> Self {
        Self {
            captured: RefCell::new(None),
        }
    }

    /// Drain the last capture.
    ///
    /// Returns the rendered plan text plus the format it was written in, or
    /// `None` while the writer awaits a fresh capture.
    pub fn take(&self) -> Option<(String, PlanFormat)> {
        self.captured.borrow_mut().take()
    }
}

impl PlanWriter for MemoryWriter {
    /// Serialize `plan` into the in-memory capture; `dest` satisfies
    /// signature parity.
    fn write(&self, plan: &Plan, _dest: &Path, format: PlanFormat) -> Result<()> {
        *self.captured.borrow_mut() = Some((format.serialize(plan)?, format));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use super::super::traits::StateEntry;

    fn empty_plan() -> Plan {
        Plan {
            version: 1,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "/home/tester/confit".into(),
            profile: "desktop".into(),
            artifacts: Vec::new(),
            hooks: Vec::new(),
        }
    }

    #[test]
    fn state_load_returns_clone() {
        let mut artifacts = BTreeMap::new();
        artifacts.insert(
            "rc:/home/tester/.bashrc".into(),
            StateEntry {
                data_hash: "abc".into(),
                output_hash: "def".into(),
                data: None,
            },
        );
        let store = MemoryState::with_state(State { artifacts });
        let loaded = store.load().unwrap();
        assert_eq!(loaded.artifacts["rc:/home/tester/.bashrc"].data_hash, "abc");
        assert!(MemoryState::new().load().unwrap().artifacts.is_empty());
    }

    #[test]
    fn writer_captures_rendered_and_format() {
        let writer = MemoryWriter::new();
        let plan = empty_plan();
        writer
            .write(&plan, Path::new("/nowhere/plan.json"), PlanFormat::Json)
            .unwrap();
        let (text, format) = writer.take().expect("write captures");
        assert_eq!(format, PlanFormat::Json);
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
        assert!(writer.take().is_none());
    }
}
