//! Drift between recorded documents and disk readers.

use std::collections::BTreeMap;
use std::path::PathBuf;

use confit_core::document::{ManifestData, ManifestDocument, render_mode};
use confit_core::drift::{Drift, DriftOrder};
use confit_core::handles::{BlobHandle, Route};
use confit_core::plan::{Bundle, opaque_label};

use crate::Applier;
use crate::snapshot::{Snapshot, TreeMemberSnapshot, drain};

/// Chunk size for streaming drift comparison.
const COMPARE_CHUNK: usize = 8192;

impl Applier {
    /// Reports manual edits between recorded documents and disk.
    ///
    /// Link documents compare target text. Entries follow
    /// recorded destination order. Content compares streaming.
    pub fn drift(&self, bundle: &Bundle, order: DriftOrder) -> Vec<Drift> {
        let mut out = Vec::new();
        for document in &bundle.manifest.documents {
            if let Some(members) = document.data.tree_members() {
                out.extend(self.tree_drift(
                    &document.destination,
                    members,
                    &self.snapshot_tree(document),
                    order,
                ));
                continue;
            }
            let Some(mut recorded) = self.recorded_reader(document) else {
                continue;
            };
            match self.snapshot_doc(document) {
                Snapshot::Absent => out.push(Drift::Missing {
                    path: document.destination.clone(),
                }),
                Snapshot::Unreadable { reason } => out.push(Drift::Unreadable {
                    path: document.destination.clone(),
                    reason,
                }),
                Snapshot::Present { mut reader, mode } => {
                    if streams_equal(&mut recorded, &mut reader) {
                        if let Some(wanted) = document.mode()
                            && let Some(seen) = mode
                            && wanted != seen
                        {
                            let (old, new) = match order {
                                DriftOrder::RecordedFirst => (
                                    serde_json::Value::String(render_mode(wanted)),
                                    serde_json::Value::String(render_mode(seen)),
                                ),
                                DriftOrder::DiskFirst => (
                                    serde_json::Value::String(render_mode(seen)),
                                    serde_json::Value::String(render_mode(wanted)),
                                ),
                            };
                            out.push(Drift::Key {
                                path: document.destination.clone(),
                                key: "mode".to_string(),
                                old: Some(old),
                                new: Some(new),
                            });
                        }
                        continue;
                    }
                    let Some(recorded_bytes) = self.recorded_bytes(document) else {
                        continue;
                    };
                    let Some(disk_bytes) = self.disk_bytes(document) else {
                        continue;
                    };
                    out.extend(document.disk_drift(&recorded_bytes, &disk_bytes, mode, order));
                }
            }
        }
        out
    }

    /// Collects drift entries for one tree destination walk.
    ///
    /// Disk extras stay quiet; hand-placed files never drift.
    fn tree_drift(
        &self,
        destination: &Route,
        members: &[confit_core::document::ManifestMember],
        disk: &BTreeMap<String, TreeMemberSnapshot>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        let mut out = Vec::new();
        for member in members {
            let member_path = destination.join(&member.relative);
            if self.member_reader(&member.blob).is_none() {
                continue;
            }
            match disk.get(&member.relative) {
                None => out.push(Drift::Missing { path: member_path }),
                Some(TreeMemberSnapshot::Unreadable { reason }) => out.push(Drift::Unreadable {
                    path: member_path,
                    reason: reason.clone(),
                }),
                Some(TreeMemberSnapshot::Present { mode, .. }) => {
                    let seen_mode = *mode;
                    let disk_snapshot = self.snapshot_tree_member(destination, &member.relative);
                    let Some(mut disk_reader) = disk_snapshot else {
                        continue;
                    };
                    let mut recorded = match self.member_reader(&member.blob) {
                        Some(reader) => reader,
                        None => continue,
                    };
                    if streams_equal(&mut recorded, &mut disk_reader) {
                        if let Some(seen) = seen_mode
                            && seen != member.mode
                        {
                            let (old, new) = match order {
                                DriftOrder::RecordedFirst => (
                                    serde_json::Value::String(render_mode(member.mode)),
                                    serde_json::Value::String(render_mode(seen)),
                                ),
                                DriftOrder::DiskFirst => (
                                    serde_json::Value::String(render_mode(seen)),
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
                        continue;
                    }
                    let _ = recorded;
                    let Some(recorded_bytes) = self.member_bytes(&member.blob) else {
                        continue;
                    };
                    let Some(disk_bytes) = self.tree_member_bytes(destination, &member.relative)
                    else {
                        continue;
                    };
                    if disk_bytes != recorded_bytes {
                        let (old, new) = match order {
                            DriftOrder::RecordedFirst => (
                                serde_json::Value::String(opaque_label(&recorded_bytes)),
                                serde_json::Value::String(opaque_label(&disk_bytes)),
                            ),
                            DriftOrder::DiskFirst => (
                                serde_json::Value::String(opaque_label(&disk_bytes)),
                                serde_json::Value::String(opaque_label(&recorded_bytes)),
                            ),
                        };
                        out.push(Drift::Key {
                            path: destination.clone(),
                            key: member.relative.clone(),
                            old: Some(old),
                            new: Some(new),
                        });
                    }
                    if let Some(seen) = seen_mode
                        && seen != member.mode
                    {
                        let (old, new) = match order {
                            DriftOrder::RecordedFirst => (
                                serde_json::Value::String(render_mode(member.mode)),
                                serde_json::Value::String(render_mode(seen)),
                            ),
                            DriftOrder::DiskFirst => (
                                serde_json::Value::String(render_mode(seen)),
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

    /// Opens a streaming reader for one recorded non-tree document.
    ///
    /// Inline payloads render from the manifest. Opaque
    /// payloads stream from the blob pool.
    fn recorded_reader(&self, document: &ManifestDocument) -> Option<Box<dyn std::io::Read>> {
        match &document.data {
            ManifestData::Opaque { blob, .. } => {
                let blobs = self.stores.blobs();
                blobs.open(blob).ok()
            }
            _ => document
                .render(&|route| PathBuf::from(route.display()))
                .ok()
                .map(|bytes| Box::new(std::io::Cursor::new(bytes)) as Box<dyn std::io::Read>),
        }
    }

    /// Reads recorded bytes for one non-tree document.
    ///
    /// Inline payloads render from the manifest. Opaque
    /// payloads stream from the blob pool.
    fn recorded_bytes(&self, document: &ManifestDocument) -> Option<Vec<u8>> {
        match &document.data {
            ManifestData::Opaque { blob, .. } => self.member_bytes(blob),
            _ => document
                .render(&|route| PathBuf::from(route.display()))
                .ok(),
        }
    }

    /// Opens a streaming reader for one member blob.
    fn member_reader(&self, handle: &BlobHandle) -> Option<Box<dyn std::io::Read>> {
        let blobs = self.stores.blobs();
        blobs.open(handle).ok()
    }

    /// Streams one member blob into bytes.
    fn member_bytes(&self, handle: &BlobHandle) -> Option<Vec<u8>> {
        let mut reader = self.member_reader(handle)?;
        drain(&mut reader).ok()
    }

    /// Opens a fresh streaming reader for one tree member path.
    fn snapshot_tree_member(
        &self,
        destination: &Route,
        relative: &str,
    ) -> Option<Box<dyn std::io::Read>> {
        self.disk.open_member(destination, relative)
    }

    /// Reads one document destination into bytes for detail lines.
    fn disk_bytes(&self, document: &ManifestDocument) -> Option<Vec<u8>> {
        match self.snapshot_doc(document) {
            Snapshot::Present { mut reader, .. } => drain(&mut reader).ok(),
            Snapshot::Absent | Snapshot::Unreadable { .. } => None,
        }
    }

    /// Reads one tree member path into bytes.
    fn tree_member_bytes(&self, destination: &Route, relative: &str) -> Option<Vec<u8>> {
        let mut reader = self.snapshot_tree_member(destination, relative)?;
        drain(&mut reader).ok()
    }
}

/// Compares two readers chunk by chunk without loading either side.
fn streams_equal(left: &mut dyn std::io::Read, right: &mut dyn std::io::Read) -> bool {
    let mut left_buf = [0u8; COMPARE_CHUNK];
    let mut right_buf = [0u8; COMPARE_CHUNK];
    loop {
        let left_read = match left.read(&mut left_buf) {
            Ok(read) => read,
            Err(_) => return false,
        };
        let right_read = match right.read(&mut right_buf) {
            Ok(read) => read,
            Err(_) => return false,
        };
        if left_read != right_read {
            return false;
        }
        if left_read == 0 {
            return true;
        }
        if left_buf[..left_read] != right_buf[..right_read] {
            return false;
        }
    }
}
