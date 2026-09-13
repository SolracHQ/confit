//! Diff
//!
//! Per setting plan diffs. Equal inputs yield
//! equal outputs so every comparison stays directly testable.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::model::dto::diff::ArtifactDetail;
use crate::model::dto::diff::ArtifactStatus;
use crate::model::dto::diff::ChangeKind;
use crate::model::dto::diff::ChangeLine;
use crate::model::dto::diff::DiskDetail;
use crate::model::dto::diff::EntryChange;
use crate::model::dto::diff::Sigil;
use crate::model::dto::snapshot::Snapshot;
use crate::model::state::State;
use crate::model::state::StateEntry;
use crate::model::state::artifact::Artifact;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::artifact::Table;
use crate::model::state::plan::Plan;
use crate::model::state::rc::AliasEntry;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::ProfileEntry;
use crate::presentation::{render_init, render_leaf, render_prev_init, render_var};
use crate::services::merge::walk_leaves;

/// Diffs a plan against previous state entry by entry.
///
/// # Arguments
///
/// * `plan` - the desired state.
/// * `previous` - the last apply record.
///
/// # Returns
///
/// Per-artifact details in plan order.
///
/// # Examples
/// ```rust
/// use confit::model::state::artifact::Artifact;
/// use confit::model::state::artifact::ArtifactData;
/// use confit::model::state::artifact::ArtifactKind;
/// use confit::model::state::plan::Plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::model::state::State;
/// use confit::services::diff::detail;
///
/// fn file_artifact(path: &str) -> Artifact {
///     Artifact {
///         kind: ArtifactKind::File,
///         path: path.into(),
///         data: ArtifactData::File { content: "hi".into() },
///         data_hash: "hash".into(),
///     }
/// }
/// let plan = Plan { version: PLAN_VERSION, created_at: String::new(), root: String::new(), profile: String::new(), artifacts: vec![file_artifact("a"), file_artifact("b")] };
/// let details = detail(&plan, &State::empty());
/// assert_eq!(details.len(), 2);
/// assert!(details.iter().all(|d| d.entries.iter().all(|e| matches!(e.change, confit::model::dto::diff::ChangeKind::Added { .. }))));
/// ```
pub fn detail(plan: &Plan, previous: &State) -> Vec<ArtifactDetail> {
    plan.artifacts
        .iter()
        .map(|artifact| detail_one(artifact, previous))
        .collect()
}

/// Diffs one artifact against its previous snapshot.
///
/// # Arguments
///
/// * `artifact` - the desired artifact.
/// * `previous` - the last apply record.
///
/// # Returns
///
/// Detail for the artifact.
fn detail_one(artifact: &Artifact, previous: &State) -> ArtifactDetail {
    let key = artifact.key_string();
    let previous_entry = previous.artifacts.get(&key);
    let status = match previous_entry {
        None => ArtifactStatus::Create,
        Some(entry) if entry.data_hash == artifact.data_hash => ArtifactStatus::Unchanged,
        Some(_) => ArtifactStatus::Update,
    };
    let snapshot = previous_entry.and_then(|entry| entry.data.as_ref());
    let entries = match &artifact.data {
        ArtifactData::Rc(rc) => detail_rc(rc, snapshot),
        ArtifactData::Toml(table) => detail_table(table, snapshot_tag(snapshot, "toml")),
        ArtifactData::Json(table) => detail_table(table, snapshot_tag(snapshot, "json")),
        ArtifactData::Yaml(table) => detail_table(table, snapshot_tag(snapshot, "yaml")),
        ArtifactData::Template { src, vars } => {
            detail_template(src, vars, snapshot_tag(snapshot, "template"))
        }
        ArtifactData::File { .. } => {
            detail_singleton(artifact, previous_entry, snapshot.is_some(), "content")
        }
        ArtifactData::Link { .. } => {
            detail_singleton(artifact, previous_entry, snapshot.is_some(), "target")
        }
    };
    ArtifactDetail {
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
            if name != Some(entry.name.as_str()) {
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
        && let Some(previous) = map.get(&entry.name).and_then(Value::as_str)
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
            if name != Some(entry.name.as_str()) {
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
            if name != Some(entry.name.as_str()) {
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
            let kept = rc.aliases.iter().any(|entry| entry.name == *name);
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

/// Diffs rc artifacts against a snapshot.
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
        let label = format!("alias {}", entry.name);
        let matched =
            match_alias_slot(entry, prev_alias_items, prev_aliases, &matched_prev_aliases);
        if empty {
            entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: entry.value.clone(),
                },
            });
            continue;
        }
        match matched {
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: entry.value.clone(),
                },
            }),
            Some((previous, prev_index)) => {
                if prev_index != usize::MAX {
                    matched_prev_aliases[prev_index] = true;
                }
                if previous == entry.value {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Unchanged {
                            value: entry.value.clone(),
                        },
                    })
                } else {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Changed {
                            from: previous,
                            to: entry.value.clone(),
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
                label: entry.name.clone(),
                change: ChangeKind::Added {
                    value: format!("\"{}\"", entry.value),
                },
            }),
            Some((previous, prev_index)) => {
                matched_prev_env[prev_index] = true;
                if previous == entry.value {
                    entries.push(EntryChange {
                        label: entry.name.clone(),
                        change: ChangeKind::Unchanged {
                            value: format!("\"{}\"", entry.value),
                        },
                    });
                } else {
                    entries.push(EntryChange {
                        label: entry.name.clone(),
                        change: ChangeKind::Changed {
                            from: format!("\"{previous}\""),
                            to: format!("\"{}\"", entry.value),
                        },
                    });
                }
            }
        }
    }

    // Profile, declaration order; same (name, when) identity as env.
    let mut matched_prev_profile = vec![false; prev_profile.map_or(0, Vec::len)];
    for entry in rc.profile.iter() {
        let label = format!("profile {}", entry.name);
        let matched = match_profile_slot(entry, prev_profile, &matched_prev_profile);
        match matched {
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: format!("\"{}\"", entry.value),
                },
            }),
            Some((previous, prev_index)) => {
                matched_prev_profile[prev_index] = true;
                if previous == entry.value {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Unchanged {
                            value: format!("\"{}\"", entry.value),
                        },
                    });
                } else {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Changed {
                            from: format!("\"{previous}\""),
                            to: format!("\"{}\"", entry.value),
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

/// Diffs table artifacts against a snapshot.
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

/// Diffs template artifacts against a snapshot.
///
/// # Arguments
///
/// * `src` - the desired template source.
/// * `vars` - the desired template variables.
/// * `snapshot` - the stored snapshot value.
///
/// # Returns
///
/// Entry changes with variables first and the source line last.
fn detail_template(
    src: &str,
    vars: &BTreeMap<String, Value>,
    snapshot: Option<&Value>,
) -> Vec<EntryChange> {
    let mut entries = Vec::new();
    let empty = snapshot.is_none();
    let (prev_src, prev_vars) = snapshot
        .and_then(Value::as_object)
        .map(|object| {
            let src = object
                .get("src")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let vars = object
                .get("vars")
                .and_then(Value::as_object)
                .map(|map| {
                    map.iter()
                        .map(|(key, value)| (key.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default();
            (src, vars)
        })
        .unwrap_or_default();
    let prev_vars: BTreeMap<String, Value> = prev_vars;
    for (key, value) in vars {
        let label = format!("vars.{key}");
        match prev_vars.get(key) {
            None if empty => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: render_var(value),
                },
            }),
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: render_var(value),
                },
            }),
            Some(prev) if prev == value => entries.push(EntryChange {
                label,
                change: ChangeKind::Unchanged {
                    value: render_var(value),
                },
            }),
            Some(prev) => entries.push(EntryChange {
                label,
                change: ChangeKind::Changed {
                    from: render_var(prev),
                    to: render_var(value),
                },
            }),
        }
    }
    if empty {
        entries.push(EntryChange {
            label: "src".to_string(),
            change: ChangeKind::Added {
                value: src.to_string(),
            },
        });
    } else if prev_src == src {
        entries.push(EntryChange {
            label: "src".to_string(),
            change: ChangeKind::Unchanged {
                value: src.to_string(),
            },
        });
    } else {
        entries.push(EntryChange {
            label: "src".to_string(),
            change: ChangeKind::Changed {
                from: prev_src,
                to: src.to_string(),
            },
        });
    }
    if !empty {
        let mut removed: Vec<EntryChange> = prev_vars
            .keys()
            .filter(|key| !vars.contains_key(*key))
            .map(|key| EntryChange {
                label: format!("vars.{key}"),
                change: ChangeKind::Removed {
                    value: render_var(&prev_vars[key]),
                },
            })
            .collect();
        removed.sort_by(|left, right| left.label.cmp(&right.label));
        // Keep `src` before removed vars? Insert removed before src line.
        let src_line = entries.pop();
        entries.extend(removed);
        if let Some(line) = src_line {
            entries.push(line);
        }
    }
    entries
}

/// Diffs file and link artifacts as one hash line.
///
/// # Arguments
///
/// * `artifact` - the desired artifact.
/// * `previous_entry` - the stored state entry.
/// * `has_snapshot` - whether stored data exists.
/// * `label` - the display label.
///
/// # Returns
///
/// Single entry change carrying the hash transition.
fn detail_singleton(
    artifact: &Artifact,
    previous_entry: Option<&StateEntry>,
    has_snapshot: bool,
    label: &str,
) -> Vec<EntryChange> {
    let current = short_hash(&artifact.data_hash);
    match previous_entry {
        None => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Added { value: current },
        }],
        Some(_) if !has_snapshot => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Added { value: current },
        }],
        Some(previous) if previous.data_hash == artifact.data_hash => vec![EntryChange {
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
/// * `rendered` - baseline bytes per artifact keyed by `kind:path`.
/// * `snapshots` - disk states per artifact keyed by `kind:path`.
///
/// # Returns
///
/// Disk details in plan order for artifacts holding both disk and baseline bytes.
pub fn detail_disk(
    plan: &Plan,
    rendered: &BTreeMap<String, Vec<u8>>,
    snapshots: &BTreeMap<String, Snapshot>,
) -> Vec<DiskDetail> {
    let mut out = Vec::new();
    for artifact in &plan.artifacts {
        let key = artifact.key_string();
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
        let lines = match &artifact.data {
            ArtifactData::Toml(table) => {
                structured_disk_lines(ArtifactKind::Toml, table, &desired_text, &disk_text)
            }
            ArtifactData::Json(table) => {
                structured_disk_lines(ArtifactKind::Json, table, &desired_text, &disk_text)
            }
            ArtifactData::Yaml(table) => {
                structured_disk_lines(ArtifactKind::Yaml, table, &desired_text, &disk_text)
            }
            ArtifactData::Rc(_) => rc_disk_lines(&desired_text, &disk_text),
            ArtifactData::Template { .. } | ArtifactData::File { .. } => {
                unified_lines(&disk_text, &desired_text)
            }
            ArtifactData::Link { target } => link_disk_lines(target, &disk_text),
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

/// Parses disk text to a table for one structured kind.
///
/// # Arguments
///
/// * `kind` - the structured kind.
/// * `text` - the on-disk text.
///
/// # Returns
///
/// Table for valid object syntax. Invalid syntax yields `None`.
fn parse_disk_table(kind: ArtifactKind, text: &str) -> Option<Table> {
    let value = match kind {
        ArtifactKind::Toml => {
            let parsed: toml::Value = toml::from_str(text).ok()?;
            serde_json::to_value(parsed).ok()?
        }
        ArtifactKind::Json => serde_json::from_str(text).ok()?,
        ArtifactKind::Yaml => noyalib::from_str(text).ok()?,
        _ => return None,
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

/// Diffs one structured table artifact.
///
/// # Arguments
///
/// * `kind` - the structured kind.
/// * `desired` - the desired table.
/// * `desired_text` - the desired baseline text.
/// * `disk_text` - the on-disk text.
///
/// # Returns
///
/// Keyed change lines for valid disk syntax; unified hunks otherwise.
fn structured_disk_lines(
    kind: ArtifactKind,
    desired: &Table,
    desired_text: &str,
    disk_text: &str,
) -> Vec<ChangeLine> {
    match parse_disk_table(kind, disk_text) {
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

/// Diffs one rc artifact.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::state::plan::PLAN_VERSION;
    use crate::model::state::rc::InitEntry;

    #[test]
    fn link_disk_lines_compare_targets_only() {
        assert_eq!(
            link_disk_lines("/new/target", "/old/target"),
            vec![ChangeLine::keyed(
                Sigil::Update,
                "target",
                Some("/old/target"),
                Some("/new/target"),
            )]
        );
        assert!(link_disk_lines("/same", "/same").is_empty());
    }

    #[test]
    fn flatten_matches_merge_leaf_paths() {
        let table: BTreeMap<String, Value> =
            serde_json::from_value(serde_json::json!({"tools": {"bat": "latest"}, "list": [1, 2]}))
                .unwrap();
        let flat = flatten_table(&table);
        assert_eq!(
            flat.get("tools.bat"),
            Some(&serde_json::json!("latest")),
            "{flat:?}"
        );
        assert_eq!(flat.get("list[0]"), Some(&serde_json::json!(1)), "{flat:?}");
        assert_eq!(flat.get("list[1]"), Some(&serde_json::json!(2)), "{flat:?}");
    }

    #[test]
    fn init_renders_shell_text() {
        let eval = InitEntry::Eval {
            argv: vec!["zoxide".into(), "init".into(), "bash".into()],
            when: None,
            priority: 0,
        };
        assert_eq!(render_init(&eval), "eval \"$(zoxide init bash)\"");
        let cmd = InitEntry::Cmd {
            argv: vec!["task".into(), "--completion".into()],
            when: None,
            priority: 0,
        };
        assert_eq!(render_init(&cmd), "task --completion");
        let source = InitEntry::Source {
            path: "~/.cargo/env".into(),
            when: None,
            priority: 0,
        };
        assert_eq!(render_init(&source), "source ~/.cargo/env");
        let prev = serde_json::json!({"eval": {"argv": ["zoxide", "init", "bash"]}});
        assert_eq!(
            render_prev_init(&prev).as_deref(),
            Some("eval \"$(zoxide init bash)\"")
        );
        let prev = serde_json::json!({"source": {"path": "~/.cargo/env"}});
        assert_eq!(
            render_prev_init(&prev).as_deref(),
            Some("source ~/.cargo/env")
        );
    }

    #[test]
    fn empty_previous_yields_all_added() {
        let artifact = Artifact {
            kind: crate::model::state::artifact::ArtifactKind::Toml,
            path: "p".into(),
            data: ArtifactData::Toml(serde_json::from_value(serde_json::json!({"a": 1})).unwrap()),
            data_hash: "new".into(),
        };
        let plan = Plan {
            version: PLAN_VERSION,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "r".into(),
            profile: "p".into(),
            artifacts: vec![artifact],
        };
        let details = detail(&plan, &State::empty());
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].status, ArtifactStatus::Create);
        assert!(
            details[0]
                .entries
                .iter()
                .all(|entry| matches!(entry.change, ChangeKind::Added { .. }))
        );
    }

    fn disk_plan(artifacts: Vec<Artifact>) -> Plan {
        Plan {
            version: PLAN_VERSION,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "r".into(),
            profile: "p".into(),
            artifacts,
        }
    }

    fn toml_artifact(path: &str, value: serde_json::Value) -> Artifact {
        Artifact {
            kind: crate::model::state::artifact::ArtifactKind::Toml,
            path: path.into(),
            data: ArtifactData::Toml(serde_json::from_value(value).unwrap()),
            data_hash: "hash".into(),
        }
    }

    fn file_artifact(path: &str, content: &str) -> Artifact {
        Artifact {
            kind: crate::model::state::artifact::ArtifactKind::File,
            path: path.into(),
            data: ArtifactData::File {
                content: content.into(),
            },
            data_hash: "hash".into(),
        }
    }

    fn disk_snapshot(bytes: &[u8]) -> Snapshot {
        Snapshot::Present {
            bytes: bytes.to_vec(),
            hash: crate::security::sha256_hex(bytes),
        }
    }

    #[test]
    fn structured_disk_diff_emits_key_lines() {
        let plan = disk_plan(vec![toml_artifact(
            "s",
            serde_json::json!({"a": 1, "b": 2}),
        )]);
        let rendered = [("toml:s".to_string(), b"a = 1\nb = 2\n".to_vec())]
            .into_iter()
            .collect();
        let snapshots = [("toml:s".to_string(), disk_snapshot(b"a = 9\nc = true\n"))]
            .into_iter()
            .collect();
        let details = detail_disk(&plan, &rendered, &snapshots);
        assert_eq!(details.len(), 1);
        assert_eq!(details[0].key, "toml:s");
        assert_eq!(
            details[0].lines,
            vec![
                ChangeLine::keyed(Sigil::Update, "a", Some("9"), Some("1")),
                ChangeLine::keyed(Sigil::Add, "b", None, Some("2")),
                ChangeLine::keyed(Sigil::Remove, "c", Some("true"), None),
            ]
        );
    }

    #[test]
    fn structured_disk_diff_parses_json_and_yaml() {
        let json_plan = disk_plan(vec![Artifact {
            kind: crate::model::state::artifact::ArtifactKind::Json,
            path: "j".into(),
            data: ArtifactData::Json(
                serde_json::from_value(serde_json::json!({"k": "v"})).unwrap(),
            ),
            data_hash: "hash".into(),
        }]);
        let rendered = [("json:j".to_string(), b"{}".to_vec())]
            .into_iter()
            .collect();
        let snapshots = [("json:j".to_string(), disk_snapshot(b"{\"k\": \"old\"}"))]
            .into_iter()
            .collect();
        let details = detail_disk(&json_plan, &rendered, &snapshots);
        assert_eq!(
            details[0].lines,
            vec![ChangeLine::keyed(
                Sigil::Update,
                "k",
                Some("old"),
                Some("v")
            )]
        );

        let yaml_plan = disk_plan(vec![Artifact {
            kind: crate::model::state::artifact::ArtifactKind::Yaml,
            path: "y".into(),
            data: ArtifactData::Yaml(serde_json::from_value(serde_json::json!({"k": 1})).unwrap()),
            data_hash: "hash".into(),
        }]);
        let rendered = [("yaml:y".to_string(), b"k: 1\n".to_vec())]
            .into_iter()
            .collect();
        let snapshots = [("yaml:y".to_string(), disk_snapshot(b"k: 2\n"))]
            .into_iter()
            .collect();
        let details = detail_disk(&yaml_plan, &rendered, &snapshots);
        assert_eq!(
            details[0].lines,
            vec![ChangeLine::keyed(Sigil::Update, "k", Some("2"), Some("1"))]
        );
    }

    #[test]
    fn unparsable_disk_falls_back_to_unified() {
        let plan = disk_plan(vec![toml_artifact("s", serde_json::json!({"a": 1}))]);
        let rendered = [("toml:s".to_string(), b"a = 1\n".to_vec())]
            .into_iter()
            .collect();
        let snapshots = [("toml:s".to_string(), disk_snapshot(b"{{{broken"))]
            .into_iter()
            .collect();
        let details = detail_disk(&plan, &rendered, &snapshots);
        assert_eq!(details[0].lines[0], ChangeLine::header("--- on disk"));
        assert_eq!(details[0].lines[1], ChangeLine::header("+++ desired"));
    }

    #[test]
    fn identical_bytes_yield_empty_lines() {
        let plan = disk_plan(vec![file_artifact("f", "same")]);
        let rendered = [("file:f".to_string(), b"same".to_vec())]
            .into_iter()
            .collect();
        let snapshots = [("file:f".to_string(), disk_snapshot(b"same"))]
            .into_iter()
            .collect();
        let details = detail_disk(&plan, &rendered, &snapshots);
        assert_eq!(details.len(), 1);
        assert!(details[0].lines.is_empty());
    }

    #[test]
    fn missing_disk_or_baseline_stays_silent() {
        let plan = disk_plan(vec![
            file_artifact("a", "x"),
            file_artifact("b", "y"),
            file_artifact("c", "z"),
        ]);
        let rendered = [
            ("file:a".to_string(), b"x".to_vec()),
            ("file:b".to_string(), b"y".to_vec()),
        ]
        .into_iter()
        .collect();
        let snapshots = [
            ("a".to_string(), Snapshot::Absent),
            (
                "b".to_string(),
                Snapshot::Unreadable {
                    reason: "cannot read 'b': denied".into(),
                },
            ),
            ("c".to_string(), disk_snapshot(b"other")),
        ]
        .into_iter()
        .collect();
        let details = detail_disk(&plan, &rendered, &snapshots);
        assert!(details.is_empty(), "{details:?}");
    }

    #[test]
    fn rc_text_parses_symmetrically_and_ignores_markers() {
        let desired = "# >>> confit:tool\nexport A=1\nalias cat=bat\n# <<< confit\n";
        assert!(rc_disk_lines(desired, desired).is_empty());
        let edited = "# >>> confit:other\nexport A=2\nalias cat=bat\n# <<< confit\n";
        assert_eq!(
            rc_disk_lines(desired, edited),
            vec![ChangeLine::keyed(Sigil::Update, "A", Some("2"), Some("1"))]
        );
        let added = "# >>> confit:tool\nexport A=1\nexport B=2\nalias cat=bat\n# <<< confit\n";
        assert_eq!(
            rc_disk_lines(added, desired),
            vec![ChangeLine::keyed(Sigil::Add, "B", None, Some("2"))]
        );
        let removed = "# >>> confit:tool\nalias cat=bat\n# <<< confit\n";
        assert_eq!(
            rc_disk_lines(desired, removed),
            vec![ChangeLine::keyed(Sigil::Add, "A", None, Some("1"))]
        );
    }

    #[test]
    fn rc_opaque_lines_diff_as_set_members() {
        let desired = "eval \"$(zoxide init bash)\"\n";
        let disk = "eval \"$(mise activate bash)\"\n";
        assert_eq!(
            rc_disk_lines(desired, disk),
            vec![
                ChangeLine::text(Sigil::Remove, "eval \"$(mise activate bash)\""),
                ChangeLine::text(Sigil::Add, "eval \"$(zoxide init bash)\""),
            ]
        );
    }

    #[test]
    fn unified_diff_shapes_single_change() {
        assert!(unified_lines("a\n", "a\n").is_empty());
        let lines = unified_lines("a\nb\nc\n", "a\nB\nc\n");
        assert_eq!(
            lines,
            vec![
                ChangeLine::header("--- on disk"),
                ChangeLine::header("+++ desired"),
                ChangeLine::header("@@ -1,3 +1,3 @@"),
                ChangeLine::text(Sigil::Context, "a"),
                ChangeLine::text(Sigil::Remove, "b"),
                ChangeLine::text(Sigil::Add, "B"),
                ChangeLine::text(Sigil::Context, "c"),
            ]
        );
    }

    #[test]
    fn unified_diff_shapes_add_only_range() {
        let lines = unified_lines("", "x\ny\n");
        assert_eq!(
            lines,
            vec![
                ChangeLine::header("--- on disk"),
                ChangeLine::header("+++ desired"),
                ChangeLine::header("@@ -0,0 +1,2 @@"),
                ChangeLine::text(Sigil::Add, "x"),
                ChangeLine::text(Sigil::Add, "y"),
            ]
        );
    }

    #[test]
    fn unified_diff_splits_distant_hunks() {
        let disk: Vec<String> = (1..=20).map(|n| format!("line {n}")).collect();
        let mut baseline = disk.clone();
        baseline[1] = "CHANGED".into();
        baseline[17] = "CHANGED".into();
        let lines = unified_lines(&disk.join("\n"), &baseline.join("\n"));
        let headers: Vec<&ChangeLine> = lines
            .iter()
            .filter(|line| line.is_header() && line.key.starts_with("@@"))
            .collect();
        assert_eq!(headers.len(), 2, "{lines:?}");
    }

    #[test]
    fn change_line_builders_shape_items() {
        let keyed = ChangeLine::keyed(Sigil::Update, "a", Some("1"), Some("2"));
        assert_eq!(keyed.sigil, Sigil::Update);
        assert_eq!(keyed.old.as_deref(), Some("1"));
        assert_eq!(keyed.new.as_deref(), Some("2"));
        assert!(!keyed.is_header());
        let header = ChangeLine::header("--- on disk");
        assert!(header.is_header());
        assert_eq!(header.sigil, Sigil::Header);
    }
}
