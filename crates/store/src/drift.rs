//! Drift
//!
//! Recorded manifest comparison against workspace disk.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::PathBuf;

use confit_core::document::render_mode;
use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::Result;
use confit_core::fs::snapshot::TreeMemberRead;
use confit_core::handles::Route;
use confit_core::ids::ReadOutcome;
use confit_core::plan::{Bundle, opaque_label};

use super::blob::BlobStore;
use super::workspace::Workspace;

/// Reports manual edits between recorded documents and disk.
///
/// The workspace sees whole destinations, so link documents
/// compare target text while every other kind compares
/// bytes from behind disk symlinks. Secret documents stay
/// quiet while present; their bytes arrive at apply time
/// alone. Entries arrive in recorded destination order.
///
/// # Arguments
///
/// * `bundle` - the recorded bundle under comparing.
/// * `workspace` - the managed disk under snapshotting.
/// * `blobs` - the blob pool under recorded bytes.
/// * `order` - the side order under assigning old and new.
///
/// # Returns
///
/// Drift entries in recorded destination order.
pub fn drift(
    bundle: &Bundle,
    workspace: &dyn Workspace,
    blobs: &dyn BlobStore,
    order: DriftOrder,
) -> Vec<Drift> {
    let mut out = Vec::new();
    for document in &bundle.manifest.documents {
        if let Some(members) = document.data.tree_members() {
            out.extend(tree_drift(
                &document.destination,
                members,
                blobs,
                &workspace.snapshot_tree(document),
                order,
            ));
            continue;
        }
        if document.is_secret() {
            match workspace.snapshot_doc(document) {
                ReadOutcome::Absent => out.push(Drift::Missing {
                    path: document.destination.clone(),
                }),
                ReadOutcome::Unreadable { reason } => out.push(Drift::Unreadable {
                    path: document.destination.clone(),
                    reason,
                }),
                ReadOutcome::Present { .. } => {}
            }
            continue;
        }
        let recorded_bytes = match recorded_bytes(document, blobs) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        match workspace.snapshot_doc(document) {
            ReadOutcome::Absent => out.push(Drift::Missing {
                path: document.destination.clone(),
            }),
            ReadOutcome::Unreadable { reason } => out.push(Drift::Unreadable {
                path: document.destination.clone(),
                reason,
            }),
            ReadOutcome::Present { bytes: disk, mode } => {
                out.extend(document.disk_drift(&recorded_bytes, &disk, mode, order));
            }
        }
    }
    out
}

/// Reads recorded bytes for one non-tree document.
///
/// Inline payloads render from the manifest. Opaque
/// payloads stream from the blob pool.
fn recorded_bytes(
    document: &confit_core::document::ManifestDocument,
    blobs: &dyn BlobStore,
) -> Result<Vec<u8>> {
    match &document.data {
        confit_core::document::ManifestData::Opaque { blob, .. } => {
            let mut reader = blobs.open(blob)?;
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map_err(|error| {
                confit_core::error::Error::Plan(format!("read blob '{}': {error}", blob.sha()))
            })?;
            Ok(bytes)
        }
        _ => document.render(&|route| PathBuf::from(route.display())),
    }
}

/// Collects drift entries for one tree destination walk.
///
/// Members compare by relative path against the disk
/// reads. Missing members report missing under their
/// joined route. Changed bytes report hash and size
/// labels under the member key. Changed modes report
/// under the member mode key. Disk extras stay quiet,
/// hand-placed files never drift.
///
/// # Arguments
///
/// * `destination` - the recorded tree destination route.
/// * `members` - the recorded tree members under comparing.
/// * `blobs` - the blob pool under recorded member bytes.
/// * `disk` - the relative disk reads under comparing.
/// * `order` - the side order under assigning old and new.
///
/// # Returns
///
/// Drift entries in manifest order.
fn tree_drift(
    destination: &Route,
    members: &[confit_core::document::ManifestMember],
    blobs: &dyn BlobStore,
    disk: &BTreeMap<String, TreeMemberRead>,
    order: DriftOrder,
) -> Vec<Drift> {
    let mut out = Vec::new();
    for member in members {
        let member_path = destination.join(&member.relative);
        let recorded = match read_member_bytes(&member.blob, blobs) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        match disk.get(&member.relative) {
            None => out.push(Drift::Missing { path: member_path }),
            Some(TreeMemberRead::Unreadable { reason }) => out.push(Drift::Unreadable {
                path: member_path,
                reason: reason.clone(),
            }),
            Some(TreeMemberRead::Present { bytes, mode }) => {
                if *bytes != recorded {
                    let (old, new) = match order {
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(opaque_label(&recorded)),
                            serde_json::Value::String(opaque_label(bytes)),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(opaque_label(bytes)),
                            serde_json::Value::String(opaque_label(&recorded)),
                        ),
                    };
                    out.push(Drift::Key {
                        path: destination.clone(),
                        key: member.relative.clone(),
                        old: Some(old),
                        new: Some(new),
                    });
                }
                if let Some(seen) = mode
                    && *seen != member.mode
                {
                    let (old, new) = match order {
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(render_mode(member.mode)),
                            serde_json::Value::String(render_mode(*seen)),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(render_mode(*seen)),
                            serde_json::Value::String(render_mode(member.mode)),
                        ),
                    };
                    out.push(Drift::Key {
                        path: destination.clone(),
                        key: format!("{}:mode", member.relative),
                        old: Some(old),
                        new: Some(new),
                    });
                }
            }
        }
    }
    out
}

/// Streams one member blob into bytes.
fn read_member_bytes(
    handle: &confit_core::handles::BlobHandle,
    blobs: &dyn BlobStore,
) -> Result<Vec<u8>> {
    let mut reader = blobs.open(handle)?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).map_err(|error| {
        confit_core::error::Error::Plan(format!("read blob '{}': {error}", handle.sha()))
    })?;
    Ok(bytes)
}
