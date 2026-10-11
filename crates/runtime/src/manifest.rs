//! Manifest
//! Persisted plans with state history.

use std::collections::BTreeSet;
use std::path::Path;

use confit_driver::sha::Sha;
use confit_model::document::{BlobRef, Document, StructuredFormat};
use confit_model::error::{Error, Result};
use confit_model::hook::Hook;
use confit_store::faults::AccessFault;
use confit_store::handles::StateHandle;
use confit_store::state::Picker;
use confit_store::state::StateStore;
use confit_store::state::error::StateError as StoreError;

use serde::{Deserialize, Serialize};

use crate::bundle::codec::{CodecError, decode};
use crate::error::{Result as RuntimeResult, RuntimeError};

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

    /// Load the applied manifest with missing states as empty.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::StateRead`] for failed state reads.
    /// - [`RuntimeError::StateCorrupt`] for bad payloads.
    /// - [`RuntimeError::StateVersion`] for unsupported versions.
    pub fn load(states: &StateStore) -> RuntimeResult<Self> {
        let handle = match states.applied() {
            Ok(handle) => handle,
            Err(StoreError::BadPick { .. }) => return Ok(Self::empty()),
            Err(error) => return Err(from_store(error)),
        };
        match states.read(&handle) {
            Ok(bytes) => decode(&bytes).map_err(|error| from_codec(handle.canonical(), error)),
            Err(StoreError::Read {
                fault: AccessFault::Missing,
                ..
            }) => Ok(Self::empty()),
            Err(error) => Err(from_store(error)),
        }
    }

    /// Write the applied state with history rotation.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::Write`] for state write faults.
    /// - [`RuntimeError::StateWriteUnknown`] for clock and other write
    ///   failures.
    /// - [`RuntimeError::StateManifest`] for manifest render failures.
    pub fn store(states: &StateStore, manifest: &Self) -> RuntimeResult<()> {
        let text = manifest
            .json()
            .map_err(|error| RuntimeError::StateManifest { source: error })?;
        let bytes = text.into_bytes();
        let picker = Picker::Applied;
        match &picker {
            Picker::Applied => {}
            Picker::Named(name) => {
                return Err(RuntimeError::BadPick {
                    input: format!("@{name}"),
                });
            }
            Picker::History(pick) => {
                return Err(RuntimeError::BadPick {
                    input: format!("%{pick}"),
                });
            }
        }
        let handle = match states.resolve(&picker) {
            Ok(handle) => handle,
            Err(StoreError::BadPick { .. }) => states.create(&picker).map_err(from_store)?,
            Err(error) => return Err(from_store(error)),
        };
        states
            .write(&handle, bytes.as_slice())
            .map_err(from_store)?;
        states.rotate(bytes.as_slice()).map_err(from_store)?;
        Ok(())
    }

    /// Resolve one picker to its stored manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BadPick`] for absent states and out-of-range
    ///   picks.
    /// - [`RuntimeError::StateRead`] for failed state reads.
    /// - [`RuntimeError::StateCorrupt`] for bad payloads.
    /// - [`RuntimeError::StateVersion`] for unsupported versions.
    pub fn resolve(states: &StateStore, picker: &Picker) -> RuntimeResult<Self> {
        let handle = states.resolve(picker).map_err(from_store)?;
        let bytes = states.read(&handle).map_err(from_store)?;
        let manifest = decode(&bytes).map_err(|error| from_codec(handle.canonical(), error))?;
        Ok(manifest)
    }

    /// Write one named state with its bundle manifest.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::BadPick`] for wrong picker kinds and bad names.
    /// - [`RuntimeError::Write`] for state write faults.
    /// - [`RuntimeError::StateWriteUnknown`] for other write failures.
    /// - [`RuntimeError::StateManifest`] for manifest render failures.
    pub fn store_named(states: &StateStore, picker: &Picker, manifest: &Self) -> RuntimeResult<()> {
        let Picker::Named(_) = picker else {
            return Err(RuntimeError::BadPick {
                input: picker_input(picker),
            });
        };
        let text = manifest
            .json()
            .map_err(|error| RuntimeError::StateManifest { source: error })?;
        let handle = match states.resolve(picker) {
            Ok(handle) => handle,
            Err(StoreError::BadPick { .. }) => states.create(picker).map_err(from_store)?,
            Err(error) => return Err(from_store(error)),
        };
        states.write(&handle, text.as_bytes()).map_err(from_store)?;
        Ok(())
    }

    /// Gather stored refs for a sweep.
    ///
    /// Every stored manifest feeds the keep set. Missing, unreadable,
    /// unparsable, and stale files add no refs.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::StateRead`] for failed folder reads.
    pub fn keep_set(states: &StateStore) -> RuntimeResult<BTreeSet<Sha>> {
        let mut keep = BTreeSet::new();
        if let Ok(handle) = states.applied() {
            collect_state_refs(&handle, states, &mut keep);
        }
        for handle in states.list().map_err(from_store)? {
            collect_state_refs(&handle, states, &mut keep);
        }
        for handle in states.list_named().map_err(from_store)? {
            collect_state_refs(&handle, states, &mut keep);
        }
        Ok(keep)
    }
}

/// Map one store failure into runtime language.
fn from_store(error: StoreError) -> RuntimeError {
    match error {
        StoreError::Read { path, fault } => RuntimeError::StateRead { path, fault },
        StoreError::Write { path, fault } => RuntimeError::Write { path, fault },
        StoreError::WriteUnknown { path, message } => {
            RuntimeError::StateWriteUnknown { path, message }
        }
        StoreError::BadPick { input } => RuntimeError::BadPick { input },
    }
}

/// Render one picker as its raw input.
fn picker_input(picker: &Picker) -> String {
    match picker {
        Picker::Applied => String::new(),
        Picker::Named(name) => format!("@{name}"),
        Picker::History(pick) => format!("%{pick}"),
    }
}

/// Map one codec failure at the state path into runtime language.
fn from_codec(path: &Path, error: CodecError) -> RuntimeError {
    match error {
        CodecError::Version { got } => RuntimeError::StateVersion {
            path: path.to_path_buf(),
            got,
        },
        CodecError::Corrupt { .. } => RuntimeError::StateCorrupt {
            path: path.to_path_buf(),
        },
    }
}

/// Collect blob refs from one stored manifest.
///
/// Unreadable, unparsable, and stale files add no refs.
fn collect_state_refs(handle: &StateHandle, states: &StateStore, keep: &mut BTreeSet<Sha>) {
    let bytes = match states.read(handle) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let stored: Manifest = match serde_json::from_slice(&bytes) {
        Ok(stored) => stored,
        Err(_) => return,
    };
    if stored.version != MANIFEST_VERSION {
        return;
    }
    keep.extend(stored.refs().iter().map(|blob| blob.stored.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::sha::Sha;
    use confit_model::document::Data;
    use confit_model::routes::{Route, RouteBase};

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
