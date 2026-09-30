//! Manifest
//!
//! Persisted plan model and manifest JSON.

use crate::document::{BlobRef, Document, StructuredFormat};
use crate::error::{Error, Result};
use crate::hook::Hook;

use serde::{Deserialize, Serialize};

/// Manifest format version written by every bundle build.
pub const MANIFEST_VERSION: u32 = 7;

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
    pub documents: Vec<Document>,
    /// Holds merged hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

impl Manifest {
    /// Builds an empty manifest at the current version.
    ///
    /// Empty manifests hold the manifest version with empty
    /// documents and hooks.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::manifest::Manifest;
    ///
    /// let manifest = Manifest::empty();
    /// assert!(manifest.documents.is_empty());
    /// ```
    pub fn empty() -> Self {
        Self {
            version: MANIFEST_VERSION,
            documents: Vec::new(),
            hooks: Vec::new(),
        }
    }

    /// Builds the desired manifest from documents.
    ///
    /// Manifests hold filled hashes in destination order with
    /// merged hooks. The caller holds one document per destination.
    ///
    /// # Errors
    ///
    /// - [`Error::Unhashable`] for documents failing hashes.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{Data, Document};
    /// use confit_model::manifest::Manifest;
    /// use confit_model::routes::{Route, RouteBase};
    ///
    /// let document = Document::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     Data::Text {
    ///         content: "hi".into(),
    ///         mode: None,
    ///         unmanaged: false,
    ///     },
    /// );
    /// let manifest = Manifest::build(vec![document], Vec::new()).unwrap();
    /// assert_eq!(manifest.documents.len(), 1);
    /// ```
    pub fn build(documents: Vec<Document>, hooks: Vec<Hook>) -> Result<Self> {
        let mut documents = documents;
        for document in &mut documents {
            document.fill_hash().map_err(|error| Error::Unhashable {
                document: document.destination.display(),
                reason: error.to_string(),
            })?;
        }
        documents.sort_by_key(|document| document.destination.display());
        Ok(Self {
            version: MANIFEST_VERSION,
            documents,
            hooks,
        })
    }

    /// Lists cloned blob refs in document order.
    pub fn refs(&self) -> Vec<BlobRef> {
        let mut refs = Vec::new();
        for document in &self.documents {
            refs.extend(document.data.blob_refs().into_iter().cloned());
        }
        refs
    }

    /// Serializes one manifest as pretty JSON.
    ///
    /// Binary bytes leave the file as blob references into the
    /// shared pool. Output bytes match pretty serde exactly.
    ///
    /// # Errors
    ///
    /// - [`Error::Render`] for serializer failures.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use confit_model::manifest::Manifest;
    ///
    /// let manifest = Manifest::empty();
    /// let text = manifest.json().unwrap();
    /// assert!(text.contains("documents"));
    /// ```
    pub fn json(&self) -> Result<String> {
        serde_json::to_string_pretty(self).map_err(|source| Error::Render {
            format: StructuredFormat::Json,
            reason: source.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Data;
    use crate::routes::{Route, RouteBase};
    use crate::sha::Sha;

    fn test_ref(content: &str, stored: &str) -> BlobRef {
        let sha = Sha::new(content).unwrap();
        let pool = Sha::new(stored).unwrap();
        BlobRef::new(sha, pool)
    }

    fn opaque_doc(name: &str, blob: BlobRef) -> Document {
        Document::new(
            Route::new(RouteBase::Home, name).unwrap(),
            Data::Opaque {
                blob,
                size: 1,
                mode: None,
                unmanaged: false,
            },
        )
    }

    #[test]
    fn refs_preserves_document_order() {
        let first = test_ref(&"11".repeat(32), &"33".repeat(32));
        let second = test_ref(&"22".repeat(32), &"44".repeat(32));
        let manifest = Manifest {
            version: MANIFEST_VERSION,
            documents: vec![
                opaque_doc("a", first.clone()),
                opaque_doc("b", second.clone()),
            ],
            hooks: Vec::new(),
        };
        let refs = manifest.refs();
        assert_eq!(refs, vec![first, second], "refs keep document order");
    }
}
