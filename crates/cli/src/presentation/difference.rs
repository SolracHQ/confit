//! Difference lines
//! Display lines over core difference entries.

use std::collections::{BTreeMap, BTreeSet};

use confit_model::routes::Route;
use confit_runtime::Difference;

/// One flattened leaf difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeafChange {
    /// The dotted leaf key.
    pub key: String,
    /// The old leaf value.
    pub old: Option<serde_json::Value>,
    /// The new leaf value.
    pub new: Option<serde_json::Value>,
}

/// Render difference entries as display lines, one or more per entry.
pub fn difference_lines(entries: &[Difference]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in entries {
        match entry {
            Difference::Key {
                path,
                key,
                old,
                new,
            } => push_key_line(&mut lines, path, key, old, new),
            Difference::Structured { path, old, new } => {
                for change in leaf_changes(old, new) {
                    push_key_line(&mut lines, path, &change.key, &change.old, &change.new);
                }
            }
            Difference::Diff { diff, .. } => {
                for line in diff.lines() {
                    if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@")
                    {
                        continue;
                    }
                    lines.push(line.to_string());
                }
            }
            Difference::Missing { path } => {
                lines.push(format!(
                    "{}: manually deleted. changed outside config: add to config or the next apply loses them",
                    path.display()
                ));
            }
            Difference::Blocked { path } => {
                lines.push(format!(
                    "'{}': directory blocks the managed path. remove the directory or apply fails",
                    path.display()
                ));
            }
            Difference::Unreadable { path, reason } => {
                lines.push(format!(
                    "cannot read '{}': {reason}. changed outside config: add to config or the next apply loses them",
                    path.display()
                ));
            }
        }
    }
    lines
}

/// Render one scalar leaf value as display text.
pub fn leaf_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

/// Flatten one map into dotted leaf entries.
pub fn flatten_table(
    value: &BTreeMap<String, serde_json::Value>,
) -> BTreeMap<String, serde_json::Value> {
    let mut out = BTreeMap::new();
    for (key, item) in value {
        flatten_value(key, item, &mut out);
    }
    out
}

/// Collect leaf differences between two maps.
pub fn leaf_changes(
    old: &BTreeMap<String, serde_json::Value>,
    new: &BTreeMap<String, serde_json::Value>,
) -> Vec<LeafChange> {
    let old_flat = flatten_table(old);
    let new_flat = flatten_table(new);
    let mut keys = BTreeSet::new();
    keys.extend(old_flat.keys().cloned());
    keys.extend(new_flat.keys().cloned());
    let mut out = Vec::new();
    for key in keys {
        let old_value = old_flat.get(&key).cloned();
        let new_value = new_flat.get(&key).cloned();
        if old_value != new_value {
            out.push(LeafChange {
                key,
                old: old_value,
                new: new_value,
            });
        }
    }
    out
}

/// Push one leaf change line for one destination.
fn push_key_line(
    lines: &mut Vec<String>,
    path: &Route,
    key: &str,
    old: &Option<serde_json::Value>,
    new: &Option<serde_json::Value>,
) {
    match (old, new) {
        (Some(old_value), Some(new_value)) => {
            lines.push(format!(
                "~ {}: {key} = {} -> {}",
                path.display(),
                leaf_text(old_value),
                leaf_text(new_value)
            ));
        }
        (Some(old_value), None) => {
            lines.push(format!(
                "- {}: {key} = {}",
                path.display(),
                leaf_text(old_value)
            ));
        }
        (None, Some(new_value)) => {
            lines.push(format!(
                "+ {}: {key} = {}",
                path.display(),
                leaf_text(new_value)
            ));
        }
        (None, None) => {}
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
