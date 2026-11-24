//! Persisted plans with state history.

use crate::document::{Data, Document};
use crate::hook::Hook;
use confit_core::error::{ContextualResult, DocumentError, ManifestError, contextual};
use confit_core::resource_ref::ResourceRef;
use confit_core::version;

use serde::{Deserialize, Serialize};

/// One persisted manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The plan format version triplet.
    pub version: [u8; 3],
    /// The documents that the plan holds, in path order.
    pub documents: Vec<Document>,
    /// The hooks in the order that the plan first sees them.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

impl Manifest {
    /// Returns the hashes in document order.
    pub fn handles(&self) -> Vec<ResourceRef> {
        let mut handles = Vec::new();
        for document in &self.documents {
            match &document.data {
                Data::Opaque(found) => handles.push(found.handle),
                Data::Tree(found) => {
                    for member in found.members.values() {
                        handles.push(*member.handle());
                    }
                }
                Data::Structured(_) | Data::Text(_) | Data::Link(_) | Data::Rc(_) => {}
            }
        }
        handles
    }

    /// Serializes one manifest to pretty JSON text.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::RenderFailed`] when the serializer fails.
    pub fn json(&self) -> ContextualResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|error| contextual(DocumentError::RenderFailed).with_detail(error.to_string()))
    }

    /// Decodes one manifest after the version check.
    ///
    /// # Errors
    ///
    /// - [`ManifestError::Invalid`] when the payload is bad.
    /// - [`ManifestError::MissingVersion`] when the version is absent.
    /// - [`ManifestError::VersionUnsupported`] when the version is incompatible.
    pub fn decode(bytes: &[u8]) -> ContextualResult<Self> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|error| contextual(ManifestError::Invalid).with_detail(error.to_string()))?;
        let raw = value
            .get("version")
            .cloned()
            .ok_or_else(|| contextual(ManifestError::MissingVersion))?;
        let found: [u8; 3] = serde_json::from_value(raw.clone()).map_err(|_| {
            contextual(ManifestError::VersionUnsupported {
                got: raw.to_string(),
            })
        })?;
        if !version::compatible(found) {
            return Err(contextual(ManifestError::VersionUnsupported {
                got: format!("[{}, {}, {}]", found[0], found[1], found[2]),
            }));
        }
        let manifest: Self = serde_json::from_value(value)
            .map_err(|error| contextual(ManifestError::Invalid).with_detail(error.to_string()))?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::error::ErrorCause;

    #[test]
    fn manifest_decode_rejects_an_older_version() {
        let raw = serde_json::json!({"version": [0, 8, 0], "documents": []});
        let bytes = serde_json::to_vec(&raw).unwrap();
        match Manifest::decode(&bytes) {
            Ok(_) => panic!("expected a version rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(got, "[0, 8, 0]", "the error keeps the version");
                    assert!(
                        error.context().is_empty(),
                        "the decode error holds no location"
                    );
                }
                other => panic!("expected a version rejection: {other}"),
            },
        }
    }

    #[test]
    fn manifest_decode_rejects_a_numeric_version() {
        let raw = serde_json::json!({"version": 9u32, "documents": []});
        let bytes = serde_json::to_vec(&raw).unwrap();
        match Manifest::decode(&bytes) {
            Ok(_) => panic!("expected a version rejection"),
            Err(error) => match error.cause() {
                ErrorCause::ManifestError(ManifestError::VersionUnsupported { got }) => {
                    assert_eq!(got, "9", "the error keeps the raw value");
                }
                other => panic!("expected a version rejection: {other}"),
            },
        }
    }
}
