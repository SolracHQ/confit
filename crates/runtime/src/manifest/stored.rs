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
use confit_core::version::{self, VERSION};

/// Loads the applied manifest and returns an empty manifest for an absent state.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a state is unreadable or the payload is bad.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the version is unsupported.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when the version is absent.
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

/// Writes the applied state and adds it to the history.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when the manifest render or the state write fails.
/// - [`confit_core::error::kind::ErrorKind::RotateHistory`] when the history rotation fails.
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

/// Resolves one picker to the manifest that its state holds.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a state is absent or unreadable,
///   or the payload is bad.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the version is unsupported.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when the version is absent.
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

/// Writes one manifest into the state that one name picks.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when the picker kind is wrong, the manifest
///   render fails, or the state write fails.
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

/// Collects the content hashes that the states hold for a sweep.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] when a folder is unreadable.
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

/// Builds one manifest with no documents at the current version.
fn empty() -> Manifest {
    Manifest {
        version: VERSION,
        documents: Vec::new(),
        hashes: BTreeMap::new(),
        hooks: Vec::new(),
    }
}

/// Writes bytes to one state handle and skips the history.
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

/// Collects the content hashes from one state manifest that decodes.
fn collect_state_refs(handle: &StateHandle, states: &StateStore, keep: &mut BTreeSet<Sha>) {
    let bytes = match states.read(handle) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let found: Manifest = match serde_json::from_slice(&bytes) {
        Ok(found) => found,
        Err(_) => return,
    };
    if !version::compatible(found.version) {
        return;
    }
    for handle in found.handles() {
        keep.insert(handle.sha().clone());
    }
}
