//! Manifest
//! Persisted plan model and manifest JSON.

use crate::document::{BlobRef, Document, StructuredFormat};
use crate::error::{Error, Result};
use crate::hook::Hook;

use serde::{Deserialize, Serialize};

/// Manifest format version for bundle builds.
pub const MANIFEST_VERSION: u32 = 7;

/// Persisted plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The plan format version.
    pub version: u32,
    /// Plan documents in path order.
    pub documents: Vec<Document>,
    /// Plan hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

impl Manifest {
    /// Build an empty manifest at the current version.
    pub fn empty() -> Self {
        Self {
            version: MANIFEST_VERSION,
            documents: Vec::new(),
            hooks: Vec::new(),
        }
    }

    /// Build the desired manifest from documents with filled hashes.
    ///
    /// - Documents sort by destination display.
    ///
    /// # Errors
    ///
    /// - [`Error::Unhashable`] for documents failing hashes.
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

    /// List cloned blob refs in document order.
    pub fn refs(&self) -> Vec<BlobRef> {
        let mut refs = Vec::new();
        for document in &self.documents {
            refs.extend(document.data.blob_refs().into_iter().cloned());
        }
        refs
    }

    /// Serialize one manifest as pretty JSON.
    ///
    /// # Errors
    ///
    /// - [`Error::Render`] for serializer failures.
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
