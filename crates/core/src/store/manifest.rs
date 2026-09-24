//! Manifest
//!
//! Persisted plan model and manifest JSON.

use crate::document::ManifestDocument;
use crate::error::{Error, Result};
use crate::hook::Hook;

use serde::{Deserialize, Serialize};

/// One stored manifest entry for the apply-past listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Holds the listing position used as the apply `%N` pick.
    pub index: usize,
}

/// Persisted plan holding metadata and blob references.
///
/// Binary bytes live gzipped in the shared pool under
/// content hashes. Text, structured, rc, and link payloads
/// stay inline. Bundles carry this shape as `manifest.json`.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Holds the plan format version.
    pub version: u32,
    /// Holds persisted documents in path order.
    pub documents: Vec<ManifestDocument>,
    /// Holds merged hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

/// Serializes one manifest as pretty JSON.
///
/// Binary bytes leave the file as blob references into the
/// shared pool. Output bytes match pretty serde exactly.
///
/// # Arguments
///
/// * `manifest` - the manifest under serializing.
///
/// # Returns
///
/// The pretty manifest JSON.
///
/// # Errors
///
/// Document serialization failures surface as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::plan::Bundle;
/// use confit_core::store::manifest::manifest_json;
///
/// let bundle = Bundle::empty();
/// assert!(matches!(manifest_json(&bundle.manifest), Ok(text) if text.contains("documents")));
/// ```
pub fn manifest_json(manifest: &Manifest) -> Result<String> {
    serde_json::to_string_pretty(manifest)
        .map_err(|error| Error::Plan(format!("render plan: {error}")))
}
