//! Stored manifests.
//! Applied, named, and history manifests in the state store.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use confit_driver::sha::Sha;
use confit_store::faults::AccessFault;
use confit_store::handles::StateHandle;
use confit_store::state::Picker;
use confit_store::state::StateStore;
use confit_store::state::error::StateError as StoreError;

use super::{CodecError, MANIFEST_VERSION, Manifest};
use crate::error::{Result as RuntimeResult, RuntimeError};

/// Load the applied manifest with missing states as empty.
///
/// # Errors
///
/// - [`RuntimeError::StateRead`] for unreadable states.
/// - [`RuntimeError::StateCorrupt`] for bad payloads.
/// - [`RuntimeError::StateVersion`] for unsupported versions.
pub fn load(states: &StateStore) -> RuntimeResult<Manifest> {
    match resolve(states, &Picker::Applied) {
        Err(RuntimeError::BadPick { .. }) => Ok(empty()),
        Err(RuntimeError::StateRead {
            fault: AccessFault::Missing,
            ..
        }) => Ok(empty()),
        other => other,
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
pub fn store(states: &StateStore, manifest: &Manifest) -> RuntimeResult<()> {
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
    store_bytes(states, &picker, &bytes)?;
    states.rotate(bytes.as_slice()).map_err(from_store)?;
    Ok(())
}

/// Resolve one picker to its sealed manifest.
///
/// # Errors
///
/// - [`RuntimeError::BadPick`] for absent states and out-of-range
///   picks.
/// - [`RuntimeError::StateRead`] for unreadable states.
/// - [`RuntimeError::StateCorrupt`] for bad payloads.
/// - [`RuntimeError::StateVersion`] for unsupported versions.
pub fn resolve(states: &StateStore, picker: &Picker) -> RuntimeResult<Manifest> {
    let handle = states.resolve(picker).map_err(from_store)?;
    let bytes = states.read(&handle).map_err(from_store)?;
    Manifest::decode(&bytes).map_err(|error| from_codec(handle.canonical(), error))
}

/// Write one named state with its bundle manifest.
///
/// # Errors
///
/// - [`RuntimeError::BadPick`] for wrong picker kinds and bad names.
/// - [`RuntimeError::Write`] for state write faults.
/// - [`RuntimeError::StateWriteUnknown`] for other write failures.
/// - [`RuntimeError::StateManifest`] for manifest render failures.
pub fn store_named(states: &StateStore, picker: &Picker, manifest: &Manifest) -> RuntimeResult<()> {
    let Picker::Named(_) = picker else {
        let input = match picker {
            Picker::Applied => String::new(),
            Picker::Named(name) => format!("@{name}"),
            Picker::History(pick) => format!("%{pick}"),
        };
        return Err(RuntimeError::BadPick { input });
    };
    let text = manifest
        .json()
        .map_err(|error| RuntimeError::StateManifest { source: error })?;
    store_bytes(states, picker, text.as_bytes())?;
    Ok(())
}

/// Gather content refs for a sweep.
///
/// Every readable state manifest feeds the keep set.
///
/// # Errors
///
/// - [`RuntimeError::StateRead`] for unreadable folders.
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

/// Build one empty manifest at the current version.
fn empty() -> Manifest {
    Manifest {
        version: MANIFEST_VERSION,
        documents: Vec::new(),
        hashes: BTreeMap::new(),
        hooks: Vec::new(),
    }
}

/// Write one state handle with history-free bytes.
fn store_bytes(states: &StateStore, picker: &Picker, bytes: &[u8]) -> RuntimeResult<()> {
    let handle = match states.resolve(picker) {
        Ok(handle) => handle,
        Err(StoreError::BadPick { .. }) => states.create(picker).map_err(from_store)?,
        Err(error) => return Err(from_store(error)),
    };
    states.write(&handle, bytes).map_err(from_store)?;
    Ok(())
}

/// Collect content refs from one readable state manifest.
fn collect_state_refs(handle: &StateHandle, states: &StateStore, keep: &mut BTreeSet<Sha>) {
    let bytes = match states.read(handle) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let found: Manifest = match serde_json::from_slice(&bytes) {
        Ok(found) => found,
        Err(_) => return,
    };
    if found.version != MANIFEST_VERSION {
        return;
    }
    for handle in found.handles() {
        keep.insert(handle.sha().clone());
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
