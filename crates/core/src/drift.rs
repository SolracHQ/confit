//! Drift
//!
//! Comparison results between recorded manifests and disk.
//!
//! Drift entries carry data alone. Drift execution rides
//! the store verb module over workspace and blob reads.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{ManifestDocument, StructuredFormat, Table};
use crate::handles::Route;
use crate::plan::opaque_label;

/// Manual edit behind one recorded destination.
///
/// Old holds recorded content. New holds disk content.
/// Missing sides read as None, so an explicit null value
/// stays distinct from an absent key.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// Structured or link leaf differs between recorded and disk.
    Key {
        /// Holds the recorded destination route.
        path: Route,
        /// Holds the dotted leaf key, link uses `target`.
        key: String,
        /// Holds the recorded leaf value, None when the key reads absent.
        old: Option<serde_json::Value>,
        /// Holds the disk leaf value, None when the key reads absent.
        new: Option<serde_json::Value>,
    },
    /// Text content differs between recorded and disk.
    Hunk {
        /// Holds the recorded destination route.
        path: Route,
        /// Holds the unified diff from recorded to disk.
        hunks: String,
    },
    /// Recorded destination reads as absent on disk.
    Missing {
        /// Holds the recorded destination route.
        path: Route,
    },
    /// Recorded destination fails to read on disk.
    Unreadable {
        /// Holds the recorded destination route.
        path: Route,
        /// Holds the raw failure detail from the read.
        reason: String,
    },
}

impl Drift {
    /// Reads the drift destination.
    ///
    /// The recorded route for key, hunk, missing, and unreadable entries.
    pub fn path(&self) -> &Route {
        match self {
            Self::Key { path, .. }
            | Self::Hunk { path, .. }
            | Self::Missing { path }
            | Self::Unreadable { path, .. } => path,
        }
    }
}

/// One drift side order selecting old/new assignment.
///
/// RecordedFirst keeps recorded bytes as old, disk as new.
/// DiskFirst keeps disk bytes as old, recorded as new.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriftOrder {
    /// Recorded bytes read as old, disk as new.
    RecordedFirst,
    /// Disk bytes read as old, recorded as new.
    DiskFirst,
}

impl ManifestDocument {
    /// Collects drift entries for one present disk path.
    ///
    /// Unmanaged documents stay quiet. Present text with
    /// matching bytes stays quiet.
    pub fn disk_drift(
        &self,
        recorded: &[u8],
        disk: &[u8],
        disk_mode: Option<u32>,
        order: DriftOrder,
    ) -> Vec<Drift> {
        use crate::document::{ManifestData, render_mode};

        let (first, second) = match order {
            DriftOrder::RecordedFirst => (recorded, disk),
            DriftOrder::DiskFirst => (disk, recorded),
        };
        let mut out = match &self.data {
            ManifestData::Structured { format, data } => {
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
            ManifestData::Text { unmanaged, .. } => {
                if *unmanaged || recorded == disk {
                    Vec::new()
                } else {
                    push_hunk(&self.destination, first, second)
                }
            }
            ManifestData::Rc(_) => {
                if recorded != disk {
                    push_hunk(&self.destination, first, second)
                } else {
                    Vec::new()
                }
            }
            ManifestData::Link { target } => {
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
            ManifestData::Opaque { unmanaged, .. } => {
                if *unmanaged || recorded == disk {
                    Vec::new()
                } else {
                    let (old, new) = match order {
                        DriftOrder::RecordedFirst => (
                            serde_json::Value::String(opaque_label(recorded)),
                            serde_json::Value::String(opaque_label(disk)),
                        ),
                        DriftOrder::DiskFirst => (
                            serde_json::Value::String(opaque_label(disk)),
                            serde_json::Value::String(opaque_label(recorded)),
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
            ManifestData::Tree { .. } => Vec::new(),
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

/// Builds render-ready hunk content from first text to second text.
///
/// File markers never leave this function.
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

/// Builds one hunk from first bytes to second bytes.
fn push_hunk(path: &Route, first: &[u8], second: &[u8]) -> Vec<Drift> {
    let old = String::from_utf8_lossy(first);
    let new = String::from_utf8_lossy(second);
    let hunks = content_hunk(&old, &new);
    vec![Drift::Hunk {
        path: path.clone(),
        hunks,
    }]
}

/// Parses disk bytes to a table by format.
fn parse_disk_table(format: StructuredFormat, disk: &[u8]) -> Option<Table> {
    let text = std::str::from_utf8(disk).ok()?;
    match format {
        StructuredFormat::Json => serde_json::from_str(text).ok(),
        StructuredFormat::Toml => toml::from_str(text).ok(),
        StructuredFormat::Yaml => noyalib::from_str(text).ok(),
    }
}

/// Diffs two tables leaf by leaf into key entries.
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

/// Flattens one table into dotted leaf entries.
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

/// Flattens one value into dotted leaf entries.
///
/// Array positions echo from 1, so printed keys match typed paths.
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
