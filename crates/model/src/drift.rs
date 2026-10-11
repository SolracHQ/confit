//! Drift
//! Comparison results between recorded manifests and disk.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{Document, StructuredFormat, Table};
use crate::routes::Route;
use confit_driver::sha::Sha;

/// Manual edit at one recorded destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
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
    /// Text difference between recorded and disk.
    Hunk {
        /// The recorded destination route.
        path: Route,
        /// The unified diff from recorded to disk.
        hunks: String,
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

/// One drift side order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftOrder {
    /// Recorded bytes as old with disk as new.
    RecordedFirst,
    /// Disk bytes as old with recorded as new.
    DiskFirst,
}

impl Document {
    /// Collect drift entries for one present disk path.
    ///
    /// - Unmanaged documents stay quiet.
    pub fn disk_drift(
        &self,
        recorded: &[u8],
        disk: &[u8],
        disk_mode: Option<u32>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        use crate::document::{Data, render_mode};

        let (first, second) = match order {
            DriftOrder::RecordedFirst => (recorded, disk),
            DriftOrder::DiskFirst => (disk, recorded),
        };
        let mut out = match &self.data {
            Data::Structured { format, data } => {
                if let Some(disk_table) = parse_disk_table(*format, disk) {
                    match order {
                        DriftOrder::RecordedFirst => {
                            structured_keys(&self.destination, data, &disk_table)
                        }
                        DriftOrder::DiskFirst => {
                            structured_keys(&self.destination, &disk_table, data)
                        }
                    }
                } else {
                    push_hunk(&self.destination, first, second)
                }
            }
            Data::Text { unmanaged, .. } => {
                if *unmanaged || recorded == disk {
                    Vec::new()
                } else {
                    push_hunk(&self.destination, first, second)
                }
            }
            Data::Rc(_) => {
                if recorded != disk {
                    push_hunk(&self.destination, first, second)
                } else {
                    Vec::new()
                }
            }
            Data::Link { target } => {
                let disk_target = String::from_utf8_lossy(disk);
                if disk_target.as_ref() != target {
                    let (old, new) = match order {
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(target.clone()),
                            serde_json::Value::String(disk_target.into_owned()),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(disk_target.into_owned()),
                            serde_json::Value::String(target.clone()),
                        ),
                    };
                    vec![Drift::Key {
                        path: self.destination.clone(),
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
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(
                                Sha::hash(recorded).label(recorded.len() as u64),
                            ),
                            serde_json::Value::String(Sha::hash(disk).label(disk.len() as u64)),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(Sha::hash(disk).label(disk.len() as u64)),
                            serde_json::Value::String(
                                Sha::hash(recorded).label(recorded.len() as u64),
                            ),
                        ),
                    };
                    vec![Drift::Key {
                        path: self.destination.clone(),
                        key: "content".to_string(),
                        old: Some(old),
                        new: Some(new),
                    }]
                }
            }
            Data::Tree { .. } => Vec::new(),
        };
        if let Some(wanted) = self.mode()
            && let Some(seen) = disk_mode
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
                path: self.destination.clone(),
                key: "mode".to_string(),
                old: Some(old),
                new: Some(new),
            });
        }
        out
    }
}

/// Build render-ready hunk content from first text to second text.
fn content_hunk(first: &str, second: &str) -> String {
    diffy::create_patch(first, second)
        .to_string()
        .lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build one hunk from first bytes to second bytes.
fn push_hunk(path: &Route, first: &[u8], second: &[u8]) -> Vec<Drift> {
    let old = String::from_utf8_lossy(first);
    let new = String::from_utf8_lossy(second);
    let hunks = content_hunk(&old, &new);
    vec![Drift::Hunk {
        path: path.clone(),
        hunks,
    }]
}

/// Parse disk bytes to a table by format.
fn parse_disk_table(format: StructuredFormat, disk: &[u8]) -> Option<Table> {
    let text = std::str::from_utf8(disk).ok()?;
    match format {
        StructuredFormat::Json => serde_json::from_str(text).ok(),
        StructuredFormat::Toml => toml::from_str(text).ok(),
        StructuredFormat::Yaml => noyalib::from_str(text).ok(),
    }
}

/// Diff two tables leaf by leaf into key entries.
fn structured_keys(path: &Route, old: &Table, new: &Table) -> Vec<Drift> {
    let mut old_flat = BTreeMap::new();
    flatten_table(old, "", &mut old_flat);
    let mut new_flat = BTreeMap::new();
    flatten_table(new, "", &mut new_flat);
    let mut keys = BTreeSet::new();
    keys.extend(old_flat.keys().cloned());
    keys.extend(new_flat.keys().cloned());
    let mut out = Vec::new();
    for key in keys {
        let old_value = old_flat.get(&key).cloned();
        let new_value = new_flat.get(&key).cloned();
        if old_value != new_value {
            out.push(Drift::Key {
                path: path.clone(),
                key: key.clone(),
                old: old_value,
                new: new_value,
            });
        }
    }
    out
}

/// Flatten one table into dotted leaf entries.
fn flatten_table(table: &Table, prefix: &str, out: &mut BTreeMap<String, serde_json::Value>) {
    for (key, value) in table {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        flatten_value(&full, value, out);
    }
}

/// Flatten one value into dotted leaf entries.
///
/// - Array positions echo from 1.
fn flatten_value(
    key: &str,
    value: &serde_json::Value,
    out: &mut BTreeMap<String, serde_json::Value>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (inner, item) in map {
                flatten_value(&format!("{key}.{inner}"), item, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_value(&format!("{key}[{}]", index + 1), item, out);
            }
        }
        _ => {
            out.insert(key.to_string(), value.clone());
        }
    }
}
