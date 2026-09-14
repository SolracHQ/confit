//! Diff
//!
//! Per setting plan diffs. Equal inputs yield
//! equal outputs so every comparison stays directly testable.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::dto::diff::ChangeKind;
use crate::model::dto::diff::ChangeLine;
use crate::model::dto::diff::DiskDetail;
use crate::model::dto::diff::DocumentDetail;
use crate::model::dto::diff::DocumentStatus;
use crate::model::dto::diff::EntryChange;
use crate::model::dto::diff::Sigil;
use crate::model::dto::snapshot::Snapshot;
use crate::model::state::State;
use crate::model::state::StateEntry;
use crate::model::state::document::Document;
use crate::model::state::document::DocumentData;
use crate::model::state::document::StructuredFormat;
use crate::model::state::document::Table;
use crate::model::state::plan::Plan;
use crate::model::state::rc::AliasEntry;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::ProfileEntry;
use crate::presentation::{render_init, render_leaf, render_prev_init};

/// Visits every leaf under a value in dotted path order.
///
/// # Arguments
///
/// * `value` - the subtree under walk.
/// * `path` - the dotted location of the subtree.
/// * `visit` - the callback for each leaf path plus leaf value.
fn walk_leaves(value: &Value, path: &str, visit: &mut impl FnMut(&str, &Value)) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                walk_leaves(item, &child, visit);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                walk_leaves(item, &format!("{path}[{index}]"), visit);
            }
        }
        _ => visit(path, value),
    }
}

/// Diffs a plan against previous state entry by entry.
///
/// # Arguments
///
/// * `plan` - the desired state.
/// * `previous` - the last apply record.
///
/// # Returns
///
/// Per-document details in plan order.
///
/// # Examples
/// ```rust
/// use confit::model::state::document::Document;
/// use confit::model::state::document::DocumentData;
/// use confit::model::state::document::DocumentKind;
/// use confit::model::state::plan::Plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::model::state::State;
/// use confit::services::diff::detail;
///
/// fn file_document(path: &str) -> Document {
///     Document {
///         kind: DocumentKind::Text,
///         path: path.into(),
///         data: DocumentData::Text { content: "hi".into() },
///         data_hash: "hash".into(),
///     }
/// }
/// let plan = Plan { version: PLAN_VERSION, created_at: String::new(), root: String::new(), profile: String::new(), documents: vec![file_document("a"), file_document("b")] };
/// let details = detail(&plan, &State::empty());
/// assert_eq!(details.len(), 2);
/// ```
pub fn detail(plan: &Plan, previous: &State) -> Vec<DocumentDetail> {
    plan.documents
        .iter()
        .map(|document| detail_one(document, previous))
        .collect()
}

/// Diffs one document against its previous snapshot.
///
/// # Arguments
///
/// * `document` - the desired document.
/// * `previous` - the last apply record.
///
/// # Returns
///
/// Detail for the document.
fn detail_one(document: &Document, previous: &State) -> DocumentDetail {
    let key = document.key_string();
    let previous_entry = previous.documents.get(&key);
    let status = match previous_entry {
        None => DocumentStatus::Create,
        Some(entry) if entry.data_hash == document.data_hash => DocumentStatus::Unchanged,
        Some(_) => DocumentStatus::Update,
    };
    let snapshot = previous_entry.and_then(|entry| entry.data.as_ref());
    let entries = match &document.data {
        DocumentData::Rc(rc) => detail_rc(rc, snapshot),
        DocumentData::Structured { data: table, .. } => {
            detail_table(table, structured_snapshot(snapshot))
        }
        DocumentData::Text { .. } => {
            detail_singleton(document, previous_entry, snapshot.is_some(), "content")
        }
        DocumentData::Link { .. } => {
            detail_singleton(document, previous_entry, snapshot.is_some(), "target")
        }
    };
    DocumentDetail {
        key,
        status,
        entries,
    }
}

/// Extracts the inner object for a tagged snapshot.
///
/// # Arguments
///
/// * `snapshot` - the stored snapshot value.
/// * `tag` - the kind tag.
///
/// # Returns
///
/// Inner object for the tag. Absent tags yield `None`.
fn snapshot_tag<'a>(snapshot: Option<&'a Value>, tag: &str) -> Option<&'a Value> {
    snapshot?.as_object()?.get(tag)
}

/// Extracts the inner data table for a structured snapshot.
///
/// # Arguments
///
/// * `snapshot` - the stored snapshot value.
///
/// # Returns
///
/// Inner data table for the structured tag. Absent tags yield `None`.
fn structured_snapshot(snapshot: Option<&Value>) -> Option<&Value> {
    snapshot_tag(snapshot, "structured")?
        .as_object()?
        .get("data")
}

/// Shortens a hash for summary display.
///
/// # Arguments
///
/// * `hash` - the full hex digest.
///
/// # Returns
///
/// Leading twelve characters of the hash.
fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

/// Flattens a JSON value into dotted leaf paths.
///
/// # Arguments
///
/// * `value` - the value under flattening.
/// * `prefix` - the dotted prefix for this value.
/// * `out` - the accumulator for leaf entries.
fn flatten_value(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
    walk_leaves(value, prefix, &mut |path, leaf| {
        out.insert(path.to_string(), leaf.clone());
    });
}

/// Flattens a table into dotted leaf paths.
///
/// # Arguments
///
/// * `table` - the table under flattening.
///
/// # Returns
///
/// Leaf values keyed by dotted path.
fn flatten_table(table: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (key, value) in table {
        flatten_value(value, key, &mut out);
    }
    out
}

/// Matches one alias entry against previous items plus the legacy map.
fn match_alias_slot(
    entry: &AliasEntry,
    prev_items: Option<&Vec<Value>>,
    prev_map: Option<&serde_json::Map<String, Value>>,
    matched: &[bool],
) -> Option<(String, usize)> {
    let desired_when = entry
        .when
        .as_ref()
        .map(|when| serde_json::to_value(when).unwrap_or(Value::Null));
    if let Some(previous) = prev_items {
        for (prev_index, item) in previous.iter().enumerate() {
            if matched.get(prev_index).is_some_and(|used| *used) {
                continue;
            }
            let object = item.as_object();
            let name = object.and_then(|o| o.get("name")).and_then(Value::as_str);
            if name != Some(entry.spec.name.as_str()) {
                continue;
            }
            let prev_when = object.and_then(|o| o.get("when")).and_then(|when| {
                if when.is_null() {
                    None
                } else {
                    Some(when.clone())
                }
            });
            if prev_when == desired_when {
                let value = object
                    .and_then(|o| o.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return Some((value, prev_index));
            }
        }
    }
    if let Some(map) = prev_map
        && entry.when.is_none()
        && let Some(previous) = map.get(&entry.spec.name).and_then(Value::as_str)
    {
        return Some((previous.to_string(), usize::MAX));
    }
    None
}

/// Matches one env entry against previous items.
fn match_env_slot(
    entry: &EnvEntry,
    prev: Option<&Vec<Value>>,
    matched: &[bool],
) -> Option<(String, usize)> {
    let desired_when = entry
        .when
        .as_ref()
        .map(|when| serde_json::to_value(when).unwrap_or(Value::Null));
    if let Some(previous) = prev {
        for (prev_index, item) in previous.iter().enumerate() {
            if matched.get(prev_index).is_some_and(|used| *used) {
                continue;
            }
            let object = item.as_object();
            let name = object.and_then(|o| o.get("name")).and_then(Value::as_str);
            if name != Some(entry.spec.name.as_str()) {
                continue;
            }
            let prev_when = object.and_then(|o| o.get("when")).and_then(|when| {
                if when.is_null() {
                    None
                } else {
                    Some(when.clone())
                }
            });
            if prev_when == desired_when {
                let value = object
                    .and_then(|o| o.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return Some((value, prev_index));
            }
        }
    }
    None
}

/// Matches one profile entry against previous items.
fn match_profile_slot(
    entry: &ProfileEntry,
    prev: Option<&Vec<Value>>,
    matched: &[bool],
) -> Option<(String, usize)> {
    let desired_when = entry
        .when
        .as_ref()
        .map(|when| serde_json::to_value(when).unwrap_or(Value::Null));
    if let Some(previous) = prev {
        for (prev_index, item) in previous.iter().enumerate() {
            if matched.get(prev_index).is_some_and(|used| *used) {
                continue;
            }
            let object = item.as_object();
            let name = object.and_then(|o| o.get("name")).and_then(Value::as_str);
            if name != Some(entry.spec.name.as_str()) {
                continue;
            }
            let prev_when = object.and_then(|o| o.get("when")).and_then(|when| {
                if when.is_null() {
                    None
                } else {
                    Some(when.clone())
                }
            });
            if prev_when == desired_when {
                let value = object
                    .and_then(|o| o.get("value"))
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                return Some((value, prev_index));
            }
        }
    }
    None
}

/// Matches one init index between desired plus previous shell text.
fn match_init_index(
    index: usize,
    desired: Option<&String>,
    previous: Option<&Option<String>>,
    empty: bool,
) -> Option<EntryChange> {
    let label = format!("init[{index}]");
    match (desired, previous) {
        (Some(desired), _) if empty => Some(EntryChange {
            label,
            change: ChangeKind::Added {
                value: desired.clone(),
            },
        }),
        (Some(desired), Some(Some(previous))) if desired == previous => Some(EntryChange {
            label,
            change: ChangeKind::Unchanged {
                value: desired.clone(),
            },
        }),
        (Some(desired), Some(Some(previous))) => Some(EntryChange {
            label,
            change: ChangeKind::Changed {
                from: previous.clone(),
                to: desired.clone(),
            },
        }),
        (Some(desired), _) => Some(EntryChange {
            label,
            change: ChangeKind::Added {
                value: desired.clone(),
            },
        }),
        (None, Some(Some(previous))) => Some(EntryChange {
            label,
            change: ChangeKind::Removed {
                value: previous.clone(),
            },
        }),
        (None, _) => None,
    }
}

/// Collects previous-only alias plus env plus profile entries.
#[allow(clippy::too_many_arguments)]
fn collect_removed(
    rc: &crate::model::state::rc::RcData,
    prev_aliases: Option<&serde_json::Map<String, Value>>,
    prev_alias_items: Option<&Vec<Value>>,
    prev_env: Option<&Vec<Value>>,
    prev_profile: Option<&Vec<Value>>,
    matched_aliases: &[bool],
    matched_env: &[bool],
    matched_profile: &[bool],
) -> Vec<EntryChange> {
    let mut removed = Vec::new();
    if let Some(map) = prev_aliases {
        for (name, value) in map {
            let kept = rc.aliases.iter().any(|entry| entry.spec.name == *name);
            if !kept {
                removed.push(EntryChange {
                    label: format!("alias {name}"),
                    change: ChangeKind::Removed {
                        value: value.as_str().unwrap_or(&render_leaf(value)).to_string(),
                    },
                });
            }
        }
    }
    if let Some(previous) = prev_alias_items {
        for (prev_index, item) in previous.iter().enumerate() {
            if matched_aliases.get(prev_index).is_some_and(|used| *used) {
                continue;
            }
            let object = item.as_object();
            let name = object
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let value = object
                .and_then(|o| o.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("");
            removed.push(EntryChange {
                label: format!("alias {name}"),
                change: ChangeKind::Removed {
                    value: value.to_string(),
                },
            });
        }
    }
    if let Some(previous) = prev_env {
        for (prev_index, item) in previous.iter().enumerate() {
            if matched_env.get(prev_index).is_some_and(|used| *used) {
                continue;
            }
            let object = item.as_object();
            let name = object
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let value = object
                .and_then(|o| o.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("");
            removed.push(EntryChange {
                label: name.to_string(),
                change: ChangeKind::Removed {
                    value: format!("\"{value}\""),
                },
            });
        }
    }
    if let Some(previous) = prev_profile {
        for (prev_index, item) in previous.iter().enumerate() {
            if matched_profile.get(prev_index).is_some_and(|used| *used) {
                continue;
            }
            let object = item.as_object();
            let name = object
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            let value = object
                .and_then(|o| o.get("value"))
                .and_then(Value::as_str)
                .unwrap_or("");
            removed.push(EntryChange {
                label: format!("profile {name}"),
                change: ChangeKind::Removed {
                    value: format!("\"{value}\""),
                },
            });
        }
    }
    removed.sort_by(|left: &EntryChange, right: &EntryChange| left.label.cmp(&right.label));
    removed
}

/// Diffs rc documents against a snapshot.
///
/// # Arguments
///
/// * `rc` - the desired rc data.
/// * `snapshot` - the stored snapshot value.
///
/// # Returns
///
/// Entry changes in display order.
fn detail_rc(rc: &crate::model::state::rc::RcData, snapshot: Option<&Value>) -> Vec<EntryChange> {
    let mut entries = Vec::new();
    let empty = snapshot.is_none();
    let inner = snapshot
        .and_then(|value| value.as_object())
        .and_then(|object| object.get("rc"));
    let prev_aliases = inner
        .and_then(Value::as_object)
        .and_then(|object| object.get("aliases"))
        .and_then(Value::as_object);
    let prev_env = inner
        .and_then(Value::as_object)
        .and_then(|object| object.get("env"))
        .and_then(Value::as_array);
    let prev_profile = inner
        .and_then(Value::as_object)
        .and_then(|object| object.get("profile"))
        .and_then(Value::as_array);
    let prev_init = inner
        .and_then(Value::as_object)
        .and_then(|object| object.get("init"))
        .and_then(Value::as_array);

    // Aliases, declaration order; identity is (name, structural-when).
    let prev_alias_items: Option<&Vec<Value>> = inner
        .and_then(Value::as_object)
        .and_then(|object| object.get("aliases"))
        .and_then(Value::as_array);
    let mut matched_prev_aliases = vec![false; prev_alias_items.map_or(0, Vec::len)];
    for entry in rc.aliases.iter() {
        let label = format!("alias {}", entry.spec.name);
        let matched =
            match_alias_slot(entry, prev_alias_items, prev_aliases, &matched_prev_aliases);
        if empty {
            entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: entry.spec.value.clone(),
                },
            });
            continue;
        }
        match matched {
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: entry.spec.value.clone(),
                },
            }),
            Some((previous, prev_index)) => {
                if prev_index != usize::MAX {
                    matched_prev_aliases[prev_index] = true;
                }
                if previous == entry.spec.value {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Unchanged {
                            value: entry.spec.value.clone(),
                        },
                    })
                } else {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Changed {
                            from: previous,
                            to: entry.spec.value.clone(),
                        },
                    })
                }
            }
        }
    }

    // Env, declaration order; identity is (name, structural-when).
    let mut matched_prev_env = vec![false; prev_env.map_or(0, Vec::len)];
    for entry in rc.env.iter() {
        let matched = match_env_slot(entry, prev_env, &matched_prev_env);
        match matched {
            None => entries.push(EntryChange {
                label: entry.spec.name.clone(),
                change: ChangeKind::Added {
                    value: format!("\"{}\"", entry.spec.value),
                },
            }),
            Some((previous, prev_index)) => {
                matched_prev_env[prev_index] = true;
                if previous == entry.spec.value {
                    entries.push(EntryChange {
                        label: entry.spec.name.clone(),
                        change: ChangeKind::Unchanged {
                            value: format!("\"{}\"", entry.spec.value),
                        },
                    });
                } else {
                    entries.push(EntryChange {
                        label: entry.spec.name.clone(),
                        change: ChangeKind::Changed {
                            from: format!("\"{previous}\""),
                            to: format!("\"{}\"", entry.spec.value),
                        },
                    });
                }
            }
        }
    }

    // Profile, declaration order; same (name, when) identity as env.
    let mut matched_prev_profile = vec![false; prev_profile.map_or(0, Vec::len)];
    for entry in rc.profile.iter() {
        let label = format!("profile {}", entry.spec.name);
        let matched = match_profile_slot(entry, prev_profile, &matched_prev_profile);
        match matched {
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: format!("\"{}\"", entry.spec.value),
                },
            }),
            Some((previous, prev_index)) => {
                matched_prev_profile[prev_index] = true;
                if previous == entry.spec.value {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Unchanged {
                            value: format!("\"{}\"", entry.spec.value),
                        },
                    });
                } else {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Changed {
                            from: format!("\"{previous}\""),
                            to: format!("\"{}\"", entry.spec.value),
                        },
                    });
                }
            }
        }
    }

    // Init, index-wise compare rendered as shell text.
    let desired_init: Vec<String> = rc.init.iter().map(render_init).collect();
    let prev_init_text: Vec<Option<String>> = prev_init
        .map(|items| items.iter().map(render_prev_init).collect())
        .unwrap_or_default();
    let width = desired_init.len().max(prev_init_text.len());
    for index in 0..width {
        if let Some(change) = match_init_index(
            index,
            desired_init.get(index),
            prev_init_text.get(index),
            empty,
        ) {
            entries.push(change);
        }
    }

    // Removed: previous-only aliases, env, profile (init handled index-wise).
    if !empty {
        entries.extend(collect_removed(
            rc,
            prev_aliases,
            prev_alias_items,
            prev_env,
            prev_profile,
            &matched_prev_aliases,
            &matched_prev_env,
            &matched_prev_profile,
        ));
    }
    entries
}

/// Diffs table documents against a snapshot.
///
/// # Arguments
///
/// * `table` - the desired table.
/// * `snapshot` - the stored snapshot value.
///
/// # Returns
///
/// Entry changes in sorted path order.
fn detail_table(table: &BTreeMap<String, Value>, snapshot: Option<&Value>) -> Vec<EntryChange> {
    let desired = flatten_table(table);
    let previous = snapshot
        .and_then(Value::as_object)
        .map(|object| {
            let mut map = BTreeMap::new();
            for (key, value) in object {
                flatten_value(value, key, &mut map);
            }
            map
        })
        .unwrap_or_default();
    let empty = snapshot.is_none();
    let mut entries = Vec::new();
    for (path, value) in &desired {
        match previous.get(path) {
            None => entries.push(EntryChange {
                label: path.clone(),
                change: ChangeKind::Added {
                    value: render_leaf(value),
                },
            }),
            Some(prev) if prev == value => entries.push(EntryChange {
                label: path.clone(),
                change: ChangeKind::Unchanged {
                    value: render_leaf(value),
                },
            }),
            Some(prev) => entries.push(EntryChange {
                label: path.clone(),
                change: ChangeKind::Changed {
                    from: render_leaf(prev),
                    to: render_leaf(value),
                },
            }),
        }
    }
    if !empty {
        let mut removed = Vec::new();
        for (path, value) in &previous {
            if !desired.contains_key(path) {
                removed.push(EntryChange {
                    label: path.clone(),
                    change: ChangeKind::Removed {
                        value: render_leaf(value),
                    },
                });
            }
        }
        entries.extend(removed);
    }
    entries
}

/// Diffs text and link documents as one hash line.
///
/// # Arguments
///
/// * `document` - the desired document.
/// * `previous_entry` - the stored state entry.
/// * `has_snapshot` - whether stored data exists.
/// * `label` - the display label.
///
/// # Returns
///
/// Single entry change carrying the hash transition.
fn detail_singleton(
    document: &Document,
    previous_entry: Option<&StateEntry>,
    has_snapshot: bool,
    label: &str,
) -> Vec<EntryChange> {
    let current = short_hash(&document.data_hash);
    match previous_entry {
        None => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Added { value: current },
        }],
        Some(_) if !has_snapshot => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Added { value: current },
        }],
        Some(previous) if previous.data_hash == document.data_hash => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Unchanged { value: current },
        }],
        Some(previous) => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Changed {
                from: short_hash(&previous.data_hash),
                to: current,
            },
        }],
    }
}

/// Diffs on-disk bytes against baseline rendering.
///
/// # Arguments
///
/// * `plan` - selects kinds and order.
/// * `rendered` - baseline bytes per document keyed by `kind:path`.
/// * `snapshots` - disk states per document keyed by `kind:path`.
///
/// # Returns
///
/// Disk details in plan order for documents holding both disk and baseline bytes.
pub fn detail_disk(
    plan: &Plan,
    rendered: &BTreeMap<String, Vec<u8>>,
    snapshots: &BTreeMap<String, Snapshot>,
) -> Vec<DiskDetail> {
    let mut out = Vec::new();
    for document in &plan.documents {
        let key = document.key_string();
        let disk_bytes = match snapshots.get(&key) {
            Some(Snapshot::Present { bytes, .. }) => bytes,
            _ => continue,
        };
        let desired_bytes = match rendered.get(&key) {
            Some(bytes) => bytes,
            None => continue,
        };
        let desired_text = String::from_utf8_lossy(desired_bytes);
        let disk_text = String::from_utf8_lossy(disk_bytes);
        let lines = match &document.data {
            DocumentData::Structured { format, data } => {
                structured_disk_lines(*format, data, &desired_text, &disk_text)
            }
            DocumentData::Rc(_) => rc_disk_lines(&desired_text, &disk_text),
            DocumentData::Text { .. } => unified_lines(&disk_text, &desired_text),
            DocumentData::Link { target } => link_disk_lines(target, &disk_text),
        };
        out.push(DiskDetail { key, lines });
    }
    out
}

/// Compares link targets between disk and desired.
///
/// # Arguments
///
/// * `target` - the desired link target.
/// * `disk_text` - the on-disk target text.
///
/// # Returns
///
/// Update item for differing targets; empty list for equal targets.
fn link_disk_lines(target: &str, disk_text: &str) -> Vec<ChangeLine> {
    if target == disk_text {
        return Vec::new();
    }
    vec![ChangeLine::keyed(
        Sigil::Update,
        "target",
        Some(disk_text),
        Some(target),
    )]
}

/// Yields unified hunks between disk and baseline text.
///
/// # Arguments
///
/// * `disk_text` - the on-disk text.
/// * `baseline_text` - the desired baseline text.
///
/// # Returns
///
/// Header plus hunk lines. Identical texts yield the empty list.
fn unified_lines(disk_text: &str, baseline_text: &str) -> Vec<ChangeLine> {
    let patch = diffy::create_patch(disk_text, baseline_text);
    if patch.hunks().is_empty() {
        return Vec::new();
    }
    let mut out = vec![
        ChangeLine::header("--- on disk"),
        ChangeLine::header("+++ desired"),
    ];
    for hunk in patch.hunks() {
        out.push(ChangeLine::header(&format!(
            "@@ -{} +{} @@",
            hunk.old_range(),
            hunk.new_range()
        )));
        for line in hunk.lines() {
            match line {
                diffy::Line::Context(body) => {
                    out.push(ChangeLine::text(Sigil::Context, trim_newline(body)));
                }
                diffy::Line::Delete(body) => {
                    out.push(ChangeLine::text(Sigil::Remove, trim_newline(body)));
                }
                diffy::Line::Insert(body) => {
                    out.push(ChangeLine::text(Sigil::Add, trim_newline(body)));
                }
            }
        }
    }
    out
}

/// Trims one trailing line ending.
///
/// # Arguments
///
/// * `body` - the line body.
///
/// # Returns
///
/// Body stripped of one trailing newline plus one trailing carriage return.
fn trim_newline(body: &str) -> &str {
    let body = body.strip_suffix('\n').unwrap_or(body);
    body.strip_suffix('\r').unwrap_or(body)
}

/// Parses disk text to a table for one structured format.
///
/// # Arguments
///
/// * `format` - the structured format.
/// * `text` - the on-disk text.
///
/// # Returns
///
/// Table for valid object syntax. Invalid syntax yields `None`.
fn parse_disk_table(format: StructuredFormat, text: &str) -> Option<Table> {
    let value = match format {
        StructuredFormat::Toml => {
            let parsed: toml::Value = toml::from_str(text).ok()?;
            serde_json::to_value(parsed).ok()?
        }
        StructuredFormat::Json => serde_json::from_str(text).ok()?,
        StructuredFormat::Yaml => noyalib::from_str(text).ok()?,
    };
    match value {
        Value::Object(map) => Some(map.into_iter().collect()),
        _ => None,
    }
}

/// Diffs flattened disk and desired tables.
///
/// # Arguments
///
/// * `disk` - the flattened disk table.
/// * `desired` - the flattened desired table.
///
/// # Returns
///
/// Keyed change lines in sorted key order.
fn key_diff_lines(
    disk: &BTreeMap<String, Value>,
    desired: &BTreeMap<String, Value>,
) -> Vec<ChangeLine> {
    let mut keys = BTreeSet::new();
    keys.extend(disk.keys().cloned());
    keys.extend(desired.keys().cloned());
    let mut lines = Vec::new();
    for key in keys {
        match (disk.get(&key), desired.get(&key)) {
            (Some(from), Some(to)) if from != to => lines.push(ChangeLine::keyed(
                Sigil::Update,
                &key,
                Some(&render_leaf(from)),
                Some(&render_leaf(to)),
            )),
            (None, Some(to)) => lines.push(ChangeLine::keyed(
                Sigil::Add,
                &key,
                None,
                Some(&render_leaf(to)),
            )),
            (Some(from), None) => lines.push(ChangeLine::keyed(
                Sigil::Remove,
                &key,
                Some(&render_leaf(from)),
                None,
            )),
            _ => {}
        }
    }
    lines
}

/// Diffs one structured table document.
///
/// # Arguments
///
/// * `format` - the structured format.
/// * `desired` - the desired table.
/// * `desired_text` - the desired baseline text.
/// * `disk_text` - the on-disk text.
///
/// # Returns
///
/// Keyed change lines for valid disk syntax; unified hunks otherwise.
fn structured_disk_lines(
    format: StructuredFormat,
    desired: &Table,
    desired_text: &str,
    disk_text: &str,
) -> Vec<ChangeLine> {
    match parse_disk_table(format, disk_text) {
        Some(disk_table) => {
            let disk_flat = flatten_table(&disk_table);
            let desired_flat = flatten_table(desired);
            key_diff_lines(&disk_flat, &desired_flat)
        }
        None => unified_lines(disk_text, desired_text),
    }
}

/// Parses shell text into keyed entries plus opaque lines.
///
/// # Arguments
///
/// * `text` - the shell text.
///
/// # Returns
///
/// Keyed entries plus opaque lines.
fn parse_rc_text(text: &str) -> (BTreeMap<String, String>, BTreeSet<String>) {
    let mut keyed = BTreeMap::new();
    let mut opaque = BTreeSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("# >>> confit") || line.starts_with("# <<< confit") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
            keyed.insert(name.to_string(), value.to_string());
        } else if let Some(rest) = line.strip_prefix("alias ") {
            let (name, value) = rest.split_once('=').unwrap_or((rest, ""));
            keyed.insert(format!("alias {name}"), value.to_string());
        } else {
            opaque.insert(line.to_string());
        }
    }
    (keyed, opaque)
}

/// Diffs one rc document.
///
/// # Arguments
///
/// * `desired_text` - the desired shell text.
/// * `disk_text` - the on-disk shell text.
///
/// # Returns
///
/// Change lines for keyed entries plus opaque lines.
fn rc_disk_lines(desired_text: &str, disk_text: &str) -> Vec<ChangeLine> {
    let (desired_keyed, desired_opaque) = parse_rc_text(desired_text);
    let (disk_keyed, disk_opaque) = parse_rc_text(disk_text);
    let mut lines = Vec::new();
    let mut keys = BTreeSet::new();
    keys.extend(disk_keyed.keys().cloned());
    keys.extend(desired_keyed.keys().cloned());
    for key in keys {
        match (disk_keyed.get(&key), desired_keyed.get(&key)) {
            (Some(from), Some(to)) if from != to => {
                lines.push(ChangeLine::keyed(Sigil::Update, &key, Some(from), Some(to)));
            }
            (None, Some(to)) => {
                lines.push(ChangeLine::keyed(Sigil::Add, &key, None, Some(to)));
            }
            (Some(from), None) => {
                lines.push(ChangeLine::keyed(Sigil::Remove, &key, Some(from), None));
            }
            _ => {}
        }
    }
    for line in disk_opaque.difference(&desired_opaque) {
        lines.push(ChangeLine::text(Sigil::Remove, line));
    }
    for line in desired_opaque.difference(&disk_opaque) {
        lines.push(ChangeLine::text(Sigil::Add, line));
    }
    lines
}
