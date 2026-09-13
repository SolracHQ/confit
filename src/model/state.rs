//! State
//!
//! Everything the app generates from Lua and applies: the domain entities
//! plus the persisted apply record shared by planning and diffing.

pub mod artifact;
pub mod condition;
pub mod config;
pub mod plan;
pub mod rc;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Persisted apply record: last-written hashes per artifact.
///
/// Key format is `"kind:path"` with the lowercase kind name, e.g. `"rc:/home/tester/.bashrc"`
/// or `"toml:/home/tester/.config/mise/config.toml"`.
///
/// Keys use the artifact kind's serialized (lowercase) name; keys serialize in fixed order, so
/// equal states share one bytes form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Last recorded hashes by `"kind:path"` key.
    pub artifacts: BTreeMap<String, StateEntry>,
}

impl State {
    /// Builds an empty apply record.
    ///
    /// # Returns
    ///
    /// The empty state, holding an empty artifact map.
    pub fn empty() -> Self {
        Self {
            artifacts: BTreeMap::new(),
        }
    }
}

/// Last-written hashes plus data snapshot for one artifact.
///
/// `Data_hash` tracks the desired data, `output_hash` tracks the
/// materialized bytes, `data` holds the canonical artifact-data
/// snapshot, holding `None` for state files written before snapshots exist.
/// Empty previous diffs read as full adds, so every desired entry reports added.
/// State files holding empty `data` parse through the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    /// Hex sha256 of the canonical desired data.
    pub data_hash: String,
    /// Hex sha256 of the materialized output bytes.
    pub output_hash: String,
    /// Canonical artifact-data snapshot (`serde_json::to_value` of the
    /// artifact data); `None` while snapshots are unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}
