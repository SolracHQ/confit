//! Stored manifests.
//! Applied, named, and history manifests in the state store.

use std::collections::{BTreeMap, BTreeSet};

use confit_core::error::ContextualResult;
use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    ErrorCause, FilesystemError, ManifestError, SlotReason, Terminal, TerminalResult, contextual,
    error,
};
use confit_core::sha::Sha;
use confit_store::handles::StateHandle;
use confit_store::state::Picker;
use confit_store::state::StateStore;

use super::Manifest;
use confit_core::version::{VERSION, compatible};

/// Load the applied manifest with missing states as empty.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable states and bad payloads.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unsupported versions.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for absent versions.
pub fn load(states: &StateStore) -> TerminalResult<Manifest> {
    match resolve(states, &Picker::Applied) {
        Ok(found) => Ok(found),
        Err(error) => match error.cause() {
            ErrorCause::SlotReason(_) => Ok(empty()),
            ErrorCause::FilesystemError(FilesystemError::FileMissing) => Ok(empty()),
            _ => Err(error),
        },
    }
}

/// Write the applied state with history rotation.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] for manifest render and state write faults.
/// - [`confit_core::error::kind::ErrorKind::RotateHistory`] for history rotation faults.
pub fn store(states: &StateStore, manifest: &Manifest) -> TerminalResult<()> {
    let text = manifest.json().terminal(ErrorKind::StoreState)?;
    let bytes = text.into_bytes();
    let picker = Picker::Applied;
    store_bytes(states, &picker, &bytes).terminal(ErrorKind::StoreState)?;
    states
        .rotate(bytes.as_slice())
        .terminal(ErrorKind::RotateHistory)?;
    Ok(())
}

/// Resolve one picker to its stored manifest.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for absent states, unreadable states,
///   and bad payloads.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unsupported versions.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for absent versions.
pub fn resolve(states: &StateStore, picker: &Picker) -> TerminalResult<Manifest> {
    let handle = states
        .resolve(picker)
        .terminal(ErrorKind::CorruptManifest)?;
    let bytes = states.read(&handle).terminal(ErrorKind::CorruptManifest)?;
    let path = handle.canonical().to_path_buf();
    let contextual = match Manifest::decode(&bytes) {
        Ok(found) => return Ok(found),
        Err(contextual) => contextual.with_fs_path(path),
    };
    let kind = match contextual.cause() {
        ErrorCause::ManifestError(ManifestError::VersionUnsupported { .. }) => {
            ErrorKind::ManifestVersion
        }
        ErrorCause::ManifestError(ManifestError::MissingVersion) => ErrorKind::ManifestNoVersion,
        _ => ErrorKind::CorruptManifest,
    };
    error(kind, contextual)
}

/// Write one named state with its bundle manifest.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] for wrong picker kinds, manifest render faults,
///   and state write faults.
pub fn store_named(
    states: &StateStore,
    picker: &Picker,
    manifest: &Manifest,
) -> TerminalResult<()> {
    let Picker::Named(_) = picker else {
        return error(ErrorKind::StoreState, contextual(SlotReason::BareSlot));
    };
    let text = manifest.json().terminal(ErrorKind::StoreState)?;
    store_bytes(states, picker, text.as_bytes()).terminal(ErrorKind::StoreState)?;
    Ok(())
}

/// Gather content refs for a sweep.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for unreadable folders.
pub fn keep_set(states: &StateStore) -> ContextualResult<BTreeSet<Sha>> {
    let mut keep = BTreeSet::new();
    if let Ok(handle) = states.applied() {
        collect_state_refs(&handle, states, &mut keep);
    }
    for handle in states.list()? {
        collect_state_refs(&handle, states, &mut keep);
    }
    for handle in states.list_named()? {
        collect_state_refs(&handle, states, &mut keep);
    }
    Ok(keep)
}

/// Build one empty manifest at the current version.
fn empty() -> Manifest {
    Manifest {
        version: VERSION,
        documents: Vec::new(),
        hashes: BTreeMap::new(),
        hooks: Vec::new(),
    }
}

/// Write one state handle with history-free bytes.
fn store_bytes(states: &StateStore, picker: &Picker, bytes: &[u8]) -> ContextualResult<()> {
    let handle = match states.resolve(picker) {
        Ok(handle) => handle,
        Err(contextual) => match contextual.cause() {
            ErrorCause::SlotReason(_) => states.create(picker)?,
            _ => return Err(contextual),
        },
    };
    states.write(&handle, bytes)?;
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
    if !compatible(found.version) {
        return;
    }
    for handle in found.handles() {
        keep.insert(handle.sha().clone());
    }
}
