//! Differ
//! Recorded manifests against disk readers.

use std::collections::BTreeMap;

use confit_driver::sha::Sha;
use confit_model::document::{Data, StructuredFormat, TreeMember, render_mode};
use confit_model::routes::Route;
use confit_store::Stores;
use confit_store::faults::AccessFault;

use crate::disk::{Disk, DiskMember, disk_doc, disk_tree, open_member};
use crate::manifest::{Document, Manifest};
use crate::render::render_document;

/// Manual edit at one recorded destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Difference {
    /// Leaf difference between recorded and disk.
    Key {
        /// The recorded destination route.
        path: Route,
        /// Dotted leaf key with `target` for links.
        key: String,
        /// The recorded leaf value.
        old: Option<serde_json::Value>,
        /// The disk leaf value.
        new: Option<serde_json::Value>,
    },
    /// Structured difference between recorded and disk.
    Structured {
        /// The recorded destination route.
        path: Route,
        /// The recorded value.
        old: serde_json::Value,
        /// The disk value.
        new: serde_json::Value,
    },
    /// Text difference between recorded and disk.
    Diff {
        /// The recorded destination route.
        path: Route,
        /// The unified diff from recorded to disk.
        diff: String,
    },
    /// Absent recorded destination on disk.
    Missing {
        /// The recorded destination route.
        path: Route,
    },
    /// Unreadable recorded destination on disk.
    Unreadable {
        /// The recorded destination route.
        path: Route,
        /// The fault cause for the failed read.
        reason: String,
    },
}

/// One difference side order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DifferenceOrder {
    /// Recorded bytes as old with disk as new.
    RecordedFirst,
    /// Disk bytes as old with recorded as new.
    DiskFirst,
}

/// Recorded against disk difference reader.
pub struct Differ {
    /// Read capabilities for one run.
    pub stores: Stores,
}

impl Differ {
    /// Report manual edits between recorded documents and disk.
    ///
    /// Link documents compare target text.
    ///
    /// - Entries follow recorded destination order.
    pub fn differences(&self, manifest: &Manifest, order: DifferenceOrder) -> Vec<Difference> {
        let mut out = Vec::new();
        for document in &manifest.documents {
            if let Some(members) = document.data.tree_members() {
                out.extend(self.tree_difference(
                    &document.destination,
                    members,
                    &disk_tree(document),
                    order,
                ));
                continue;
            }
            match disk_doc(document) {
                Disk::Absent => out.push(Difference::Missing {
                    path: document.destination.clone(),
                }),
                Disk::Unreadable { fault } => out.push(Difference::Unreadable {
                    path: document.destination.clone(),
                    reason: unreadable_reason(&fault),
                }),
                Disk::Present { mut reader, mode } => {
                    if !matches!(&document.data, Data::Opaque { .. }) {
                        let Some(recorded_bytes) = render_document(document).ok() else {
                            continue;
                        };
                        let mut disk_bytes = Vec::new();
                        if reader.read_to_end(&mut disk_bytes).is_err() {
                            continue;
                        }
                        out.extend(disk_difference(
                            document,
                            &recorded_bytes,
                            &disk_bytes,
                            mode,
                            order,
                        ));
                        continue;
                    }
                    if let Data::Opaque { blob, .. } = &document.data {
                        let store = self.stores.blobs();
                        let Ok(handle) = store.resolve(blob) else {
                            continue;
                        };
                        let recorded_label = match store.len(&handle) {
                            Ok(len) => blob.sha256.label(len),
                            Err(_) => continue,
                        };
                        let disk_label = match Self::disk_label(document) {
                            Some(label) => label,
                            None => continue,
                        };
                        if recorded_label == disk_label {
                            if let Some(wanted) = document.mode()
                                && let Some(seen) = mode
                                && wanted != seen
                            {
                                let (old, new) = match order {
                                    DifferenceOrder::RecordedFirst => (
                                        serde_json::Value::String(render_mode(wanted)),
                                        serde_json::Value::String(render_mode(seen)),
                                    ),
                                    DifferenceOrder::DiskFirst => (
                                        serde_json::Value::String(render_mode(seen)),
                                        serde_json::Value::String(render_mode(wanted)),
                                    ),
                                };
                                out.push(Difference::Key {
                                    path: document.destination.clone(),
                                    key: "mode".to_string(),
                                    old: Some(old),
                                    new: Some(new),
                                });
                            }
                            continue;
                        }
                        out.push(opaque_content_difference(
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

    /// Collect difference entries for one tree destination walk.
    ///
    /// - Only manifest members produce entries.
    fn tree_difference(
        &self,
        destination: &Route,
        members: &BTreeMap<String, TreeMember>,
        disk: &BTreeMap<String, DiskMember>,
        order: DifferenceOrder,
    ) -> Vec<Difference> {
        let mut out = Vec::new();
        for (relative, member) in members {
            let member_path = destination.join(relative);
            match disk.get(relative) {
                None => out.push(Difference::Missing { path: member_path }),
                Some(DiskMember::Unreadable { fault }) => out.push(Difference::Unreadable {
                    path: member_path,
                    reason: unreadable_reason(fault),
                }),
                Some(DiskMember::Present { mode, .. }) => {
                    let seen_mode = *mode;
                    let store = self.stores.blobs();
                    let Ok(handle) = store.resolve(&member.blob) else {
                        continue;
                    };
                    let recorded_label = match store.len(&handle) {
                        Ok(len) => member.blob.sha256.label(len),
                        Err(_) => continue,
                    };
                    let Some(disk_label) = Self::tree_member_label(destination, relative) else {
                        continue;
                    };
                    if recorded_label != disk_label {
                        out.push(opaque_content_difference(
                            destination,
                            relative,
                            &recorded_label,
                            &disk_label,
                            order,
                        ));
                    }
                    if let Some(seen) = seen_mode
                        && seen != member.mode
                    {
                        let (old, new) = match order {
                            DifferenceOrder::RecordedFirst => (
                                serde_json::Value::String(render_mode(member.mode)),
                                serde_json::Value::String(render_mode(seen)),
                            ),
                            DifferenceOrder::DiskFirst => (
                                serde_json::Value::String(render_mode(seen)),
                                serde_json::Value::String(render_mode(member.mode)),
                            ),
                        };
                        out.push(Difference::Key {
                            path: destination.clone(),
                            key: format!("{relative}:mode"),
                            old: Some(old),
                            new: Some(new),
                        });
                    }
                }
            }
        }
        out
    }

    /// Label one disk document with its content hash and byte count.
    fn disk_label(document: &Document) -> Option<String> {
        match disk_doc(document) {
            Disk::Present { mut reader, .. } => {
                let (sha, len) = Sha::read_with_size(&mut reader).ok()?;
                Some(sha.label(len))
            }
            Disk::Absent | Disk::Unreadable { .. } => None,
        }
    }

    /// Label one tree member path with its content hash and byte count.
    fn tree_member_label(destination: &Route, relative: &str) -> Option<String> {
        let mut reader = open_member(destination, relative)?;
        let (sha, len) = Sha::read_with_size(&mut reader).ok()?;
        Some(sha.label(len))
    }
}

/// Collect difference entries for one present disk path.
///
/// - Unmanaged documents stay quiet.
fn disk_difference(
    document: &Document,
    recorded: &[u8],
    disk: &[u8],
    disk_mode: Option<u32>,
    order: DifferenceOrder,
) -> Vec<Difference> {
    let (first, second) = match order {
        DifferenceOrder::RecordedFirst => (recorded, disk),
        DifferenceOrder::DiskFirst => (disk, recorded),
    };
    let mut out = match &document.data {
        Data::Structured { format, data } => {
            if let Some(disk_table) = parse_disk_table(*format, disk) {
                match order {
                    DifferenceOrder::RecordedFirst => {
                        structured_difference(&document.destination, data, &disk_table)
                    }
                    DifferenceOrder::DiskFirst => {
                        structured_difference(&document.destination, &disk_table, data)
                    }
                }
            } else {
                push_diff(&document.destination, first, second)
            }
        }
        Data::Text { unmanaged, .. } => {
            if *unmanaged || recorded == disk {
                Vec::new()
            } else {
                push_diff(&document.destination, first, second)
            }
        }
        Data::Rc(_) => {
            if recorded != disk {
                push_diff(&document.destination, first, second)
            } else {
                Vec::new()
            }
        }
        Data::Link { target } => {
            let disk_target = String::from_utf8_lossy(disk);
            if disk_target.as_ref() != target.display() {
                let (old, new) = match order {
                    DifferenceOrder::RecordedFirst => (
                        serde_json::Value::String(target.display()),
                        serde_json::Value::String(disk_target.into_owned()),
                    ),
                    DifferenceOrder::DiskFirst => (
                        serde_json::Value::String(disk_target.into_owned()),
                        serde_json::Value::String(target.display()),
                    ),
                };
                vec![Difference::Key {
                    path: document.destination.clone(),
                    key: "target".to_string(),
                    old: Some(old),
                    new: Some(new),
                }]
            } else {
                Vec::new()
            }
        }
        Data::Opaque { unmanaged, .. } => {
            if *unmanaged || recorded == disk {
                Vec::new()
            } else {
                let (old, new) = match order {
                    DifferenceOrder::RecordedFirst => (
                        serde_json::Value::String(Sha::hash(recorded).label(recorded.len() as u64)),
                        serde_json::Value::String(Sha::hash(disk).label(disk.len() as u64)),
                    ),
                    DifferenceOrder::DiskFirst => (
                        serde_json::Value::String(Sha::hash(disk).label(disk.len() as u64)),
                        serde_json::Value::String(Sha::hash(recorded).label(recorded.len() as u64)),
                    ),
                };
                vec![Difference::Key {
                    path: document.destination.clone(),
                    key: "content".to_string(),
                    old: Some(old),
                    new: Some(new),
                }]
            }
        }
        Data::Tree { .. } => Vec::new(),
    };
    if let Some(wanted) = document.mode()
        && let Some(seen) = disk_mode
        && wanted != seen
    {
        let (old, new) = match order {
            DifferenceOrder::RecordedFirst => (
                serde_json::Value::String(render_mode(wanted)),
                serde_json::Value::String(render_mode(seen)),
            ),
            DifferenceOrder::DiskFirst => (
                serde_json::Value::String(render_mode(seen)),
                serde_json::Value::String(render_mode(wanted)),
            ),
        };
        out.push(Difference::Key {
            path: document.destination.clone(),
            key: "mode".to_string(),
            old: Some(old),
            new: Some(new),
        });
    }
    out
}

/// Build render-ready diff content from first text to second text.
fn content_diff(first: &str, second: &str) -> String {
    diffy::create_patch(first, second)
        .to_string()
        .lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build one diff from first bytes to second bytes.
fn push_diff(path: &Route, first: &[u8], second: &[u8]) -> Vec<Difference> {
    let old = String::from_utf8_lossy(first);
    let new = String::from_utf8_lossy(second);
    let diff = content_diff(&old, &new);
    vec![Difference::Diff {
        path: path.clone(),
        diff,
    }]
}

/// Parse disk bytes to a value by format.
fn parse_disk_table(format: StructuredFormat, disk: &[u8]) -> Option<serde_json::Value> {
    let text = std::str::from_utf8(disk).ok()?;
    let value: serde_json::Value = match format {
        StructuredFormat::Json => serde_json::from_str(text).ok()?,
        StructuredFormat::Toml => toml::from_str(text).ok()?,
        StructuredFormat::Yaml => noyalib::from_str(text).ok()?,
    };
    if value.is_object() { Some(value) } else { None }
}

/// Report one path-level entry for differing structured values.
fn structured_difference(
    path: &Route,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> Vec<Difference> {
    if old == new {
        Vec::new()
    } else {
        vec![Difference::Structured {
            path: path.clone(),
            old: old.clone(),
            new: new.clone(),
        }]
    }
}

/// Build one opaque content key from two labels.
fn opaque_content_difference(
    path: &Route,
    key: &str,
    recorded: &str,
    disk: &str,
    order: DifferenceOrder,
) -> Difference {
    let (old, new) = match order {
        DifferenceOrder::RecordedFirst => (recorded, disk),
        DifferenceOrder::DiskFirst => (disk, recorded),
    };
    Difference::Key {
        path: path.clone(),
        key: key.to_string(),
        old: Some(serde_json::Value::String(old.to_string())),
        new: Some(serde_json::Value::String(new.to_string())),
    }
}

/// Show one unreadable fault for difference lines.
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
    use confit_model::document::{Data, RcData, RcEntry};
    use confit_model::routes::{Route, RouteBase};

    use crate::manifest::MANIFEST_VERSION;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "confit-runtime-difference-{}-{name}",
            std::process::id()
        ));
        driver::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn difference_renders_routes_expanded_like_for_like() {
        let _guard = TestGuard::install();
        let dir = scratch("rc");
        let dest = dir.join("shellrc");
        let sourced = dir.join("sourced.sh");
        let document = Document::new(
            literal(&dest),
            Data::Rc(RcData::new(
                vec![RcEntry::Source {
                    path: literal(&sourced),
                    when: None,
                }],
                Vec::new(),
                Vec::new(),
            )),
        );
        let recorded = render_document(&document).unwrap();
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
        let manifest = crate::manifest::Manifest {
            version: MANIFEST_VERSION,
            documents: vec![document.clone()],
            hooks: Vec::new(),
        };
        let (sender, _) = crossbeam_channel::unbounded();
        let differ = Differ {
            stores: confit_store::Stores::new(confit_store::StoreRoots::default(), sender),
        };
        let found = differ.differences(&manifest, DifferenceOrder::RecordedFirst);
        assert!(
            found.is_empty(),
            "matching expanded bytes are zero difference"
        );
        assert_eq!(
            document.destination.display(),
            dest.to_str().unwrap(),
            "literal display renders the bare host path"
        );
        driver::fs::remove_file(&dest).unwrap();
        let _ = driver::fs::remove_file(&dir);
    }
}
