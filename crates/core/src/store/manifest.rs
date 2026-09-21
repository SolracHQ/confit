//! Manifest
//!
//! Persisted plan model plus manifest JSON.

use crate::document::ManifestDocument;
use crate::error::{Error, Result};
use crate::hook::Hook;
use crate::plan::Bundle;

use serde::{Deserialize, Serialize};

/// One stored manifest entry for the apply-past listing.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Holds the listing position used as the apply `%N` pick.
    pub index: usize,
}

/// Persisted plan holding metadata plus blob references.
///
/// Binary bytes live gzipped in the shared pool under
/// content hashes. Text, structured, rc, plus link payloads
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

impl Manifest {
    /// Reads the persisted plan back as itself.
    ///
    /// The manifest is the only document language, so the
    /// projection clones the bundle manifest intact.
    ///
    /// # Arguments
    ///
    /// * `plan` - the bundle holding the manifest.
    ///
    /// # Returns
    ///
    /// The manifest for plan files plus bundles.
    ///
    pub fn of(bundle: &Bundle) -> Self {
        bundle.manifest.clone()
    }

    /// Reads one persisted manifest back as itself.
    ///
    /// # Arguments
    ///
    /// * `manifest` - the persisted plan under projecting.
    ///
    /// # Returns
    ///
    /// The manifest clone.
    pub fn of_manifest(manifest: &Manifest) -> Self {
        manifest.clone()
    }
}

/// Serializes one plan as a pretty manifest.
///
/// Binary bytes leave the file as blob references into the
/// shared pool. Output bytes match pretty serde exactly.
///
/// # Arguments
///
/// * `plan` - the plan under serializing.
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
/// assert!(matches!(manifest_json(&bundle), Ok(text) if text.contains("documents")));
/// ```
pub fn manifest_json(bundle: &Bundle) -> Result<String> {
    let stored = Manifest::of(bundle);
    serde_json::to_string_pretty(&stored)
        .map_err(|error| Error::Plan(format!("render plan: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::fs::Filesystem;
    use crate::plan::{BUNDLE_VERSION, Bundle};
    use crate::store::slots::load_state;

    #[test]
    fn parallel_plan_json_matches_sequential() {
        use crate::document::{ManifestData, ManifestDocument, StructuredFormat, Table};
        use crate::ids::DocPath;

        let opaque_bytes = vec![0xFF, 0x00, 0x41];
        let opaque_blob = crate::ids::sha256_hex(&opaque_bytes);
        let mut blobs = BTreeMap::new();
        blobs.insert(opaque_blob.clone(), opaque_bytes);
        let bundle = Bundle {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents: vec![
                    ManifestDocument::new(
                        DocPath::new("note"),
                        ManifestData::Text {
                            content: "héllo \"quoted\"\n".to_string(),
                            mode: None,
                            unmanaged: false,
                        },
                    ),
                    ManifestDocument::new(
                        DocPath::new("bin"),
                        ManifestData::Opaque {
                            blob: opaque_blob,
                            size: 3,
                            mode: None,
                            unmanaged: false,
                        },
                    ),
                    ManifestDocument::new(
                        DocPath::new("app.json"),
                        ManifestData::Structured {
                            format: StructuredFormat::Json,
                            data: Table::from([("name".to_string(), serde_json::json!("confit"))]),
                        },
                    ),
                ],
                hooks: vec![crate::hook::Hook {
                    argv: vec!["mise".to_string(), "install".to_string()],
                    path: Vec::new(),
                    requires: None,
                    when: None,
                    checks: Vec::new(),
                    timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
                }],
            },
            blobs,
        };
        let parallel = match manifest_json(&bundle) {
            Ok(text) => text,
            Err(error) => panic!("parallel serializes: {error}"),
        };
        let sequential = match serde_json::to_string_pretty(&Manifest::of(&bundle)) {
            Ok(text) => text,
            Err(error) => panic!("sequential serializes: {error}"),
        };
        assert_eq!(parallel, sequential);
        assert!(parallel.contains('\n'));
        assert!(parallel.contains("\"blob\""));
        assert!(!parallel.contains("/wBB"));
    }

    #[test]
    fn manifest_rejects_unknown_fields() {
        use crate::fs::memory::MemoryFs;

        let fs = MemoryFs::new();
        let text =
            format!("{{\"version\":{BUNDLE_VERSION},\"documents\":[],\"hooks\":[],\"extra\":1}}");
        match fs.write(std::path::Path::new("state.json"), text.as_bytes()) {
            Ok(()) => {}
            Err(error) => panic!("memory writes: {error}"),
        }
        match load_state(Some(std::path::Path::new("state.json")), &fs) {
            Ok(_) => panic!("unknown field passes"),
            Err(error) => assert!(
                error.to_string().contains("unknown field"),
                "names the field: {error}"
            ),
        }
    }
}
