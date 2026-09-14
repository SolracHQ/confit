//! State
//!
//! Everything the app generates from Lua and applies: the domain entities
//! plus the persisted apply record shared by planning and diffing.

pub mod condition;
pub mod config;
pub mod document;
pub mod level;
pub mod plan;
pub mod rc;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Persisted apply record: last-written hashes per document.
///
/// Key format is `"kind:path"` with the lowercase kind name, e.g. `"rc:/home/tester/.bashrc"`
/// or `"toml:/home/tester/.config/mise/config.toml"`.
///
/// Keys use the document kind's serialized (lowercase) name; keys serialize in fixed order, so
/// equal states share one bytes form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Last recorded hashes by `"kind:path"` key.
    pub documents: BTreeMap<String, StateEntry>,
}

impl State {
    /// Builds an empty apply record.
    ///
    /// # Returns
    ///
    /// The empty state, holding an empty document map.
    pub fn empty() -> Self {
        Self {
            documents: BTreeMap::new(),
        }
    }
}

/// Last-written hashes plus data snapshot for one document.
///
/// `Data_hash` tracks the desired data, `data` holds the canonical document-data
/// snapshot, holding `None` for state files written before snapshots exist.
/// Empty previous diffs read as full adds, so every desired entry reports added.
/// State files holding empty `data` parse through the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    /// Hex sha256 of the canonical desired data.
    pub data_hash: String,
    /// Canonical document-data snapshot (`serde_json::to_value` of the
    /// document data); `None` while snapshots are unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
