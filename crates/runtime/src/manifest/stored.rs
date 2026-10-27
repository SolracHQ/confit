//! Stored manifests.
//! Applied, named, and history manifests in the state store.

use std::collections::{BTreeMap, BTreeSet};

use confit_core::error::Result as CoreResult;
use confit_core::error::locate::Locate;
use confit_core::error::{
    Error, ErrorCause, ErrorContext, Failure, ManifestError, Mixed, Outcome, SlotReason,
};
use confit_core::sha::Sha;
use confit_store::handles::StateHandle;
use confit_store::state::Picker;
use confit_store::state::StateStore;

use super::{MANIFEST_VERSION, Manifest};

/// Load the applied manifest with missing states as empty.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for unreadable states beyond missing states.
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for bad payloads and unsupported versions.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for bad payloads and unsupported versions.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for bad payloads and unsupported versions.
pub fn load(states: &StateStore) -> Mixed<Manifest> {
    match resolve(states, &Picker::Applied) {
        Err(Failure::Mid(error)) => {
            let mut current = &error;
            while let Some(inner) = current.inner() {
                current = inner;
            }
            match current {
                ErrorContext::Leaf(ErrorCause::SlotReason(_)) => Ok(empty()),
                ErrorContext::Leaf(ErrorCause::FilesystemError(
                    confit_core::error::FilesystemError::FileMissing,
                )) => Ok(empty()),
                _ => Err(Failure::Mid(error.clone())),
            }
        }
        other => other,
    }
}

/// Write the applied state with history rotation.
///
/// # Errors
///
/// - [`confit_core::error::Cannot::store_state`] for manifest render and state write faults.
/// - [`confit_core::error::Cannot::rotate_history`] for history rotation faults.
pub fn store(states: &StateStore, manifest: &Manifest) -> Outcome<()> {
    let text = manifest.json().map_err(Error::store_state)?;
    let bytes = text.into_bytes();
    let picker = Picker::Applied;
    store_bytes(states, &picker, &bytes).map_err(Error::store_state)?;
    states
        .rotate(bytes.as_slice())
        .map_err(Error::rotate_history)?;
    Ok(())
}

/// Resolve one picker to its sealed manifest.
///
/// # Errors
///
/// - [`confit_core::error::SlotReason`] for absent states and out-of-range picks.
/// - [`confit_core::error::FilesystemError`] for unreadable states.
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for bad payloads and unsupported versions.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for bad payloads and unsupported versions.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for bad payloads and unsupported versions.
pub fn resolve(states: &StateStore, picker: &Picker) -> Mixed<Manifest> {
    let handle = states.resolve(picker)?;
    let bytes = states.read(&handle)?;
    match Manifest::decode(&bytes).path(handle.canonical()) {
        Ok(manifest) => Ok(manifest),
        Err(ErrorContext::AtPath { path, inner }) => match *inner {
            ErrorContext::Leaf(ErrorCause::ManifestError(ManifestError::Invalid { detail })) => {
                Err(Failure::Terminal(Error::corrupt_manifest(path, detail)))
            }
            ErrorContext::Leaf(ErrorCause::ManifestError(ManifestError::VersionUnsupported {
                got,
            })) => Err(Failure::Terminal(Error::manifest_version(path, got))),
            ErrorContext::Leaf(ErrorCause::ManifestError(ManifestError::MissingVersion)) => {
                Err(Failure::Terminal(Error::manifest_no_version(path)))
            }
            other => Err(Failure::Mid(ErrorContext::AtPath {
                path,
                inner: Box::new(other),
            })),
        },
        Err(other) => Err(Failure::Mid(other)),
    }
}

/// Write one named state with its bundle manifest.
///
/// # Errors
///
/// - [`confit_core::error::Cannot::store_state`] for wrong picker kinds, manifest render faults, and state write faults.
pub fn store_named(states: &StateStore, picker: &Picker, manifest: &Manifest) -> Outcome<()> {
    let Picker::Named(_) = picker else {
        return Err(Error::store_state(ErrorContext::from(
            ErrorCause::SlotReason(SlotReason::BareSlot),
        )));
    };
    let text = manifest.json().map_err(Error::store_state)?;
    store_bytes(states, picker, text.as_bytes()).map_err(Error::store_state)?;
    Ok(())
}

/// Gather content refs for a sweep.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for unreadable folders.
pub fn keep_set(states: &StateStore) -> CoreResult<BTreeSet<Sha>> {
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
        version: MANIFEST_VERSION,
        documents: Vec::new(),
        hashes: BTreeMap::new(),
        hooks: Vec::new(),
    }
}

/// Write one state handle with history-free bytes.
fn store_bytes(states: &StateStore, picker: &Picker, bytes: &[u8]) -> CoreResult<()> {
    let handle = match states.resolve(picker) {
        Ok(handle) => handle,
        Err(error) => {
            let mut current = &error;
            while let Some(inner) = current.inner() {
                current = inner;
            }
            match current {
                ErrorContext::Leaf(ErrorCause::SlotReason(_)) => states.create(picker)?,
                _ => return Err(error.clone()),
            }
        }
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
    if found.version != MANIFEST_VERSION {
        return;
    }
    for handle in found.handles() {
        keep.insert(handle.sha().clone());
    }
}
