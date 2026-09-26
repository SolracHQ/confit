//! Drift between recorded documents and disk readers.

use std::collections::BTreeMap;

use confit_model::document::{ManifestData, ManifestDocument, render_mode};
use confit_model::drift::{Drift, DriftOrder};
use confit_model::handles::{Route, Sha};
use confit_model::plan::opaque_id;
use confit_store::bundle::Bundle;

use crate::Applier;
use crate::disk::{Live, LiveMember, drain};

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
                    &self.disk.live_tree(document),
                    order,
                ));
                continue;
            }
            match self.disk.live_doc(document) {
                Live::Absent => out.push(Drift::Missing {
                    path: document.destination.clone(),
                }),
                Live::Unreadable { reason } => out.push(Drift::Unreadable {
                    path: document.destination.clone(),
                    reason,
                }),
                Live::Present { mut reader, mode } => {
                    if !matches!(&document.data, ManifestData::Opaque { .. }) {
                        let Some(recorded_bytes) = self.render_document(document).ok() else {
                            continue;
                        };
                        let Ok(disk_bytes) = drain(&mut reader) else {
                            continue;
                        };
                        out.extend(document.disk_drift(&recorded_bytes, &disk_bytes, mode, order));
                        continue;
                    }
                    if let ManifestData::Opaque { blob, .. } = &document.data {
                        let recorded_label = match self.stores.blobs().len(blob) {
                            Ok(len) => opaque_id(blob.sha(), len),
                            Err(_) => continue,
                        };
                        let disk_label = match self.disk_label(document) {
                            Some(label) => label,
                            None => continue,
                        };
                        if recorded_label == disk_label {
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
                        out.push(opaque_content_drift(
                            &document.destination,
                            "content",
                            &recorded_label,
                            &disk_label,
                            order,
                        ));
                        continue;
                    }
                }
            }
        }
        out
    }

    /// Collects drift entries for one tree destination walk.
    ///
    /// Extra disk files stay out of the entries; hand-placed
    /// files read untouched.
    fn tree_drift(
        &self,
        destination: &Route,
        members: &[confit_model::document::ManifestMember],
        disk: &BTreeMap<String, LiveMember>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        let mut out = Vec::new();
        for member in members {
            let member_path = destination.join(&member.relative);
            match disk.get(&member.relative) {
                None => out.push(Drift::Missing { path: member_path }),
                Some(LiveMember::Unreadable { reason }) => out.push(Drift::Unreadable {
                    path: member_path,
                    reason: reason.clone(),
                }),
                Some(LiveMember::Present { mode, .. }) => {
                    let seen_mode = *mode;
                    let recorded_label = match self.stores.blobs().len(&member.blob) {
                        Ok(len) => opaque_id(member.blob.sha(), len),
                        Err(_) => continue,
                    };
                    let Some(disk_label) = self.tree_member_label(destination, &member.relative)
                    else {
                        continue;
                    };
                    if recorded_label != disk_label {
                        out.push(opaque_content_drift(
                            destination,
                            &member.relative,
                            &recorded_label,
                            &disk_label,
                            order,
                        ));
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

    /// Labels one disk document with its content hash and byte count.
    fn disk_label(&self, document: &ManifestDocument) -> Option<String> {
        match self.disk.live_doc(document) {
            Live::Present { mut reader, .. } => {
                let (sha, len) = hash_count(&mut reader).ok()?;
                Some(opaque_id(&sha, len))
            }
            Live::Absent | Live::Unreadable { .. } => None,
        }
    }

    /// Labels one tree member path with its content hash and byte count.
    fn tree_member_label(&self, destination: &Route, relative: &str) -> Option<String> {
        let mut reader = self.disk.open_member(destination, relative)?;
        let (sha, len) = hash_count(&mut reader).ok()?;
        Some(opaque_id(&sha, len))
    }
}

/// Builds one opaque content key from two labels.
fn opaque_content_drift(
    path: &Route,
    key: &str,
    recorded: &str,
    disk: &str,
    order: DriftOrder,
) -> Drift {
    let (old, new) = match order {
        DriftOrder::RecordedFirst => (recorded, disk),
        DriftOrder::DiskFirst => (disk, recorded),
    };
    Drift::Key {
        path: path.clone(),
        key: key.to_string(),
        old: Some(serde_json::Value::String(old.to_string())),
        new: Some(serde_json::Value::String(new.to_string())),
    }
}

/// Hashes one reader streaming while counting bytes.
///
/// # Returns
///
/// The content hash with the byte count.
fn hash_count(reader: &mut dyn std::io::Read) -> std::io::Result<(Sha, u64)> {
    use sha2::Digest as _;

    let mut hasher = sha2::Sha256::new();
    let mut chunk = [0u8; COMPARE_CHUNK];
    let mut len: u64 = 0;
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok((Sha::finish(hasher), len));
        }
        len += read as u64;
        hasher.update(&chunk[..read]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver as driver;
    use confit_driver::TestGuard;
    use confit_model::document::{ManifestData, ManifestDocument, RcData, RcEntry, RcOp};
    use confit_model::handles::{Route, RouteBase};
    use confit_store::bundle::BUNDLE_VERSION;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "confit-runtime-drift-{}-{name}",
            std::process::id()
        ));
        driver::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn drift_renders_routes_expanded_like_for_like() {
        let _guard = TestGuard::install();
        let dir = scratch("rc");
        let dest = dir.join("shellrc");
        let sourced = dir.join("sourced.sh");
        let document = ManifestDocument::new(
            literal(&dest),
            ManifestData::Rc(RcData::new(
                vec![RcEntry {
                    op: RcOp::Source {
                        path: literal(&sourced),
                    },
                    when: None,
                }],
                Vec::new(),
                Vec::new(),
            )),
        );
        let applier = Applier::host(confit_store::StoreRoots::default());
        let recorded = applier.render_document(&document).unwrap();
        let text = String::from_utf8_lossy(&recorded).into_owned();
        assert!(
            text.contains(sourced.to_str().unwrap()),
            "rendered bytes carry the expanded path"
        );
        assert!(
            !text.contains("literal:"),
            "rendered bytes carry no display alias"
        );
        driver::write(&dest, &recorded).unwrap();
        let bundle = Bundle {
            manifest: confit_model::manifest::Manifest {
                version: BUNDLE_VERSION,
                documents: vec![document.clone()],
                hooks: Vec::new(),
            },
            blobs: std::collections::BTreeMap::new(),
        };
        let found = applier.drift(&bundle, DriftOrder::RecordedFirst);
        assert!(
            found.is_empty(),
            "matching expanded bytes read as zero drift"
        );
        assert!(
            !driver::exists(std::path::Path::new(&document.destination.display())),
            "display alias never lands on disk"
        );
        driver::remove_file(&dest).unwrap();
        let _ = driver::remove_file(&dir);
    }
}
