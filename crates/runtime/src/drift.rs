//! Drift between recorded documents and disk readers.

use std::collections::BTreeMap;

use confit_model::document::{Data, Document, render_mode};
use confit_model::drift::{Drift, DriftOrder};
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use confit_model::sha::Sha;

use crate::Applier;
use crate::disk::{Live, LiveMember, drain};
use confit_store::faults::AccessFault;

impl Applier {
    /// Reports manual edits between recorded documents and disk.
    ///
    /// Link documents compare target text. Entries follow
    /// recorded destination order. Content compares streaming.
    pub fn drift(&self, manifest: &Manifest, order: DriftOrder) -> Vec<Drift> {
        let mut out = Vec::new();
        for document in &manifest.documents {
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
                Live::Unreadable { fault } => out.push(Drift::Unreadable {
                    path: document.destination.clone(),
                    reason: unreadable_reason(&fault),
                }),
                Live::Present { mut reader, mode } => {
                    if !matches!(&document.data, Data::Opaque { .. }) {
                        let Some(recorded_bytes) = self.render_document(document).ok() else {
                            continue;
                        };
                        let Ok(disk_bytes) = drain(&mut reader) else {
                            continue;
                        };
                        out.extend(document.disk_drift(&recorded_bytes, &disk_bytes, mode, order));
                        continue;
                    }
                    if let Data::Opaque { blob, .. } = &document.data {
                        let store = self.stores.blobs();
                        let Ok(handle) = store.resolve(blob) else {
                            continue;
                        };
                        let recorded_label = match store.len(&handle) {
                            Ok(len) => blob.sha().label(len),
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
                Some(LiveMember::Unreadable { fault }) => out.push(Drift::Unreadable {
                    path: member_path,
                    reason: unreadable_reason(fault),
                }),
                Some(LiveMember::Present { mode, .. }) => {
                    let seen_mode = *mode;
                    let store = self.stores.blobs();
                    let Ok(handle) = store.resolve(&member.blob) else {
                        continue;
                    };
                    let recorded_label = match store.len(&handle) {
                        Ok(len) => member.blob.sha().label(len),
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
    fn disk_label(&self, document: &Document) -> Option<String> {
        match self.disk.live_doc(document) {
            Live::Present { mut reader, .. } => {
                let (sha, len) = Sha::read_with_size(&mut reader).ok()?;
                Some(sha.label(len))
            }
            Live::Absent | Live::Unreadable { .. } => None,
        }
    }

    /// Labels one tree member path with its content hash and byte count.
    fn tree_member_label(&self, destination: &Route, relative: &str) -> Option<String> {
        let mut reader = self.disk.open_member(destination, relative)?;
        let (sha, len) = Sha::read_with_size(&mut reader).ok()?;
        Some(sha.label(len))
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

/// Names one unreadable fault for drift lines.
fn unreadable_reason(fault: &AccessFault) -> String {
    match fault {
        AccessFault::Unknown { .. } => "unknown failure, see log".to_owned(),
        known => known.cause().to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver as driver;
    use confit_driver::fs::TestGuard;
    use confit_model::document::{Data, Document, RcData, RcEntry, RcOp};
    use confit_model::manifest::MANIFEST_VERSION;
    use confit_model::routes::{Route, RouteBase};

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "confit-runtime-drift-{}-{name}",
            std::process::id()
        ));
        driver::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn drift_renders_routes_expanded_like_for_like() {
        let _guard = TestGuard::install();
        let dir = scratch("rc");
        let dest = dir.join("shellrc");
        let sourced = dir.join("sourced.sh");
        let document = Document::new(
            literal(&dest),
            Data::Rc(RcData::new(
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
        driver::fs::write(&dest, &recorded).unwrap();
        let manifest = confit_model::manifest::Manifest {
            version: MANIFEST_VERSION,
            documents: vec![document.clone()],
            hooks: Vec::new(),
        };
        let found = applier.drift(&manifest, DriftOrder::RecordedFirst);
        assert!(
            found.is_empty(),
            "matching expanded bytes read as zero drift"
        );
        assert!(
            !driver::fs::exists(std::path::Path::new(&document.destination.display())),
            "display alias never lands on disk"
        );
        driver::fs::remove_file(&dest).unwrap();
        let _ = driver::fs::remove_file(&dir);
    }
}
