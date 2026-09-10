//! Entry-level plan diffs: per-setting changes with winner attribution.
//!
//! Compares desired plan artifacts against previous state snapshots entry by
//! entry, attributing each winner to its tool. Plan-level counts stay in
//! `crate::plan::diff`; this module adds the rich per-entry layer that
//! `summarize` renders.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::model::{Artifact, ArtifactData, InitEntry, Plan, when_eq};
use crate::store::State;

/// Per-artifact lifecycle status against the previous state.
///
/// Yields the same three outcomes as the count diff: absent previous means
/// create, differing `data_hash` means update, equal hash means unchanged.
/// Guarantees: every plan artifact maps to exactly one status; status derives
/// from `data_hash` alone, never from entry contents, so hash-equal artifacts
/// report unchanged even while entry rendering walks snapshots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    /// Artifact absent from the previous state.
    Create,
    /// Artifact present with a differing `data_hash`.
    Update,
    /// Artifact present with an equal `data_hash`.
    Unchanged,
}

/// Per-entry change shape.
///
/// Yields the transition for one labeled setting: added carries the desired
/// value, changed carries previous then desired, removed carries the previous
/// value, unchanged carries the shared value.
/// Guarantees: `Changed` always names `from` (previous) then `to` (desired);
/// file/link/fetched values hold truncated (12-char) `data_hash`es, never raw
/// bytes, so large contents stay out of the summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Setting present in desired, absent in previous; holds desired value.
    Added {
        /// Desired value (or truncated hash for file/link/fetched).
        value: String,
    },
    /// Setting present in both with differing values.
    Changed {
        /// Previous value (or previous truncated hash).
        from: String,
        /// Desired value (or current truncated hash).
        to: String,
    },
    /// Setting present in previous, absent in desired; holds previous value.
    Removed {
        /// Previous value.
        value: String,
    },
    /// Setting present in both with equal values; holds shared value.
    Unchanged {
        /// Shared value.
        value: String,
    },
}

/// One labeled entry transition with winner attribution.
///
/// Yields the label plus change plus winner/loser tools.
/// Guarantees: `tool` names the desired winner (`None` for removed entries,
/// which have no desired winner); `over` names the in-plan shadow loser when
/// shadow history names a different tool than the winner, else `None`, so the
/// conflicts flag renders losers only where history identifies one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryChange {
    /// Setting label: `alias cat`, `_ZO_DOCTOR`, `profile PATH`,
    /// `init[0]`, `tools.bat`, `vars.key`, `src`, `content`, `target`,
    /// `source`.
    pub label: String,
    /// Transition with values.
    pub change: ChangeKind,
    /// Winner tool (`None` for removed entries).
    pub tool: Option<String>,
    /// Shadow loser tool for `--conflicts` rendering.
    pub over: Option<String>,
}

/// Per-artifact entry diff.
///
/// Yields the artifact key plus status plus ordered entries.
/// Guarantees: `key` reads `"kind:path"` with the lowercase kind, matching
/// `crate::plan::diff`; `entries` hold desired order first (aliases sorted,
/// env/profile declaration order, init index order, toml dotted-path sorted,
/// template vars sorted plus `src`, file/link/fetched single line), then
/// removed entries sorted by label; absent previous or absent snapshot yields
/// all-added entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDetail {
    /// Artifact key `"kind:path"`.
    pub key: String,
    /// Lifecycle status from `data_hash` comparison.
    pub status: ArtifactStatus,
    /// Ordered entry transitions.
    pub entries: Vec<EntryChange>,
}

/// Diff a plan against previous state entry by entry.
///
/// Walks each plan artifact's desired data plus `_blame` against the previous
/// snapshot (`previous.artifacts[key].data` as a generic JSON value, never
/// deserialized back into model types). Absent state or absent snapshot reads
/// as empty previous: every desired entry reports added.
///
/// Invariants: status mirrors `crate::plan::diff` (`data_hash` compare);
/// entry values render strings raw except rc env/profile (quoted) and
/// file/link/fetched (truncated hashes); removed entries carry `tool: None`.
///
/// Args: `plan` is the desired state, `previous` the last apply record.
///
/// Example:
/// ```rust,no_run
/// use std::path::Path;
/// use confit::diff::detail;
/// use confit::lua::evaluate;
/// use confit::plan::orchestrate;
/// use confit::store::State;
///
/// let graph = evaluate(Path::new("examples/0-basic_tool"), Path::new("examples/0-basic_tool/profile.lua")).unwrap();
/// let plan = orchestrate(&graph, &State::empty(), "root", "profile").unwrap();
/// let details = detail(&plan, &State::empty());
/// assert_eq!(details.len(), 2);
/// assert!(details.iter().all(|d| d.entries.iter().all(|e| matches!(e.change, confit::diff::ChangeKind::Added { .. }))));
/// ```
pub fn detail(plan: &Plan, previous: &State) -> Vec<ArtifactDetail> {
    plan.artifacts
        .iter()
        .map(|artifact| detail_one(artifact, previous))
        .collect()
}

/// Diff one artifact against its previous snapshot.
fn detail_one(artifact: &Artifact, previous: &State) -> ArtifactDetail {
    let key = format!("{}:{}", artifact.kind, artifact.path);
    let previous_entry = previous.artifacts.get(&key);
    let status = match previous_entry {
        None => ArtifactStatus::Create,
        Some(entry) if entry.data_hash == artifact.data_hash => ArtifactStatus::Unchanged,
        Some(_) => ArtifactStatus::Update,
    };
    let snapshot = previous_entry.and_then(|entry| entry.data.as_ref());
    let entries = match &artifact.data {
        ArtifactData::Rc(rc) => detail_rc(artifact, rc, snapshot),
        ArtifactData::Toml(table) => detail_table(artifact, table, snapshot_tag(snapshot, "toml")),
        ArtifactData::Json(table) => detail_table(artifact, table, snapshot_tag(snapshot, "json")),
        ArtifactData::Yaml(table) => detail_table(artifact, table, snapshot_tag(snapshot, "yaml")),
        ArtifactData::Template { src, vars } => {
            detail_template(artifact, src, vars, snapshot_tag(snapshot, "template"))
        }
        ArtifactData::File { .. } => {
            detail_singleton(artifact, previous_entry, snapshot.is_some(), "content")
        }
        ArtifactData::Link { .. } => {
            detail_singleton(artifact, previous_entry, snapshot.is_some(), "target")
        }
        ArtifactData::Fetched { .. } => {
            detail_singleton(artifact, previous_entry, snapshot.is_some(), "source")
        }
    };
    ArtifactDetail {
        key,
        status,
        entries,
    }
}

/// Extract the inner object for a tagged snapshot: `{"toml": {...}}` -> `{...}`.
fn snapshot_tag<'a>(snapshot: Option<&'a Value>, tag: &str) -> Option<&'a Value> {
    snapshot?.as_object()?.get(tag)
}

/// Truncate a hash to 12 chars for summary display.
fn short_hash(hash: &str) -> String {
    hash.chars().take(12).collect()
}

/// Last contribution tool (last-wins kinds); `None` for empty lists.
fn last_tool(artifact: &Artifact) -> Option<String> {
    artifact
        .contributions
        .iter()
        .max_by_key(|contribution| contribution.order)
        .map(|contribution| contribution.tool.clone())
}

/// Alias `over`: most recent shadow winner differing from the winner.
fn alias_over(artifact: &Artifact, name: &str, tool: Option<&str>) -> Option<String> {
    let mut over = None;
    for shadow in &artifact.shadowed.aliases {
        if shadow.name == name
            && let Some(winner) = tool
            && shadow.winner != winner
        {
            over = Some(shadow.winner.clone());
        }
    }
    over
}

/// Render an rc init entry as shell text: `eval "$(a b)"` or plain `a b`.
fn render_init(entry: &InitEntry) -> String {
    match entry {
        InitEntry::Eval { argv } => format!("eval \"$({})\"", argv.join(" ")),
        InitEntry::Cmd { argv } => argv.join(" "),
    }
}

/// Render a generic previous init value (generic JSON walk) as shell text.
fn render_prev_init(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if let Some(eval) = object.get("eval") {
        let argv = eval.get("argv")?.as_array()?;
        let parts: Vec<String> = argv
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<_>>()?;
        return Some(format!("eval \"$({})\"", parts.join(" ")));
    }
    if let Some(cmd) = object.get("cmd") {
        let argv = cmd.get("argv")?.as_array()?;
        let parts: Vec<String> = argv
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect::<Option<_>>()?;
        return Some(parts.join(" "));
    }
    None
}

/// Render a scalar leaf: strings raw, numbers/bools compact, null as `null`.
fn render_leaf(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(_) | Value::Bool(_) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

/// Render a template var value: strings raw, everything else compact JSON.
fn render_var(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        _ => serde_json::to_string(value).unwrap_or_else(|_| value.to_string()),
    }
}

/// Flatten a JSON value into dotted leaf paths (`a.b`, arrays as `a[0]`).
fn flatten_value(value: &Value, prefix: &str, out: &mut BTreeMap<String, Value>) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                let child = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_value(item, &child, out);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_value(item, &format!("{prefix}[{index}]"), out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), value.clone());
        }
    }
}

/// Flatten a table (top-level map) into dotted leaf paths.
fn flatten_table(table: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (key, value) in table {
        flatten_value(value, key, &mut out);
    }
    out
}

/// Detail rc artifacts: profile, env, aliases, init, then removed.
fn detail_rc(
    artifact: &Artifact,
    rc: &crate::model::RcData,
    snapshot: Option<&Value>,
) -> Vec<EntryChange> {
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

    // Aliases, sorted by key (BTreeMap order).
    for (name, value) in &rc.aliases {
        let label = format!("alias {name}");
        let tool = artifact.blame.aliases.get(name).cloned();
        if empty {
            entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: value.clone(),
                },
                over: None,
                tool,
            });
            continue;
        }
        let prev_value = prev_aliases
            .and_then(|map| map.get(name))
            .and_then(Value::as_str);
        match prev_value {
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: value.clone(),
                },
                tool: tool.clone(),
                over: None,
            }),
            Some(previous) if previous == value => entries.push(EntryChange {
                label,
                change: ChangeKind::Unchanged {
                    value: value.clone(),
                },
                tool: tool.clone(),
                over: None,
            }),
            Some(previous) => entries.push(EntryChange {
                label,
                change: ChangeKind::Changed {
                    from: previous.to_string(),
                    to: value.clone(),
                },
                over: alias_over(artifact, name, tool.as_deref()),
                tool,
            }),
        }
    }

    // Env, declaration order; identity is (name, structural-when).
    let mut matched_prev_env = vec![false; prev_env.map_or(0, Vec::len)];
    for (index, entry) in rc.env.iter().enumerate() {
        let tool = artifact.blame.env.get(index).cloned();
        let desired_when = entry
            .when
            .as_ref()
            .map(|when| serde_json::to_value(when).unwrap_or(Value::Null));
        let mut matched: Option<(String, usize)> = None;
        if let Some(previous) = prev_env {
            for (prev_index, item) in previous.iter().enumerate() {
                if matched_prev_env[prev_index] {
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
                    matched = Some((value, prev_index));
                    break;
                }
            }
        }
        let over = guarded_env_over(artifact, entry, tool.as_deref());
        match matched {
            None => entries.push(EntryChange {
                label: entry.name.clone(),
                change: ChangeKind::Added {
                    value: format!("\"{}\"", entry.value),
                },
                tool,
                over: None,
            }),
            Some((previous, prev_index)) => {
                matched_prev_env[prev_index] = true;
                if previous == entry.value {
                    entries.push(EntryChange {
                        label: entry.name.clone(),
                        change: ChangeKind::Unchanged {
                            value: format!("\"{}\"", entry.value),
                        },
                        tool,
                        over: None,
                    });
                } else {
                    entries.push(EntryChange {
                        label: entry.name.clone(),
                        change: ChangeKind::Changed {
                            from: format!("\"{previous}\""),
                            to: format!("\"{}\"", entry.value),
                        },
                        tool,
                        over,
                    });
                }
            }
        }
    }

    // Profile, declaration order; same (name, when) identity as env.
    let mut matched_prev_profile = vec![false; prev_profile.map_or(0, Vec::len)];
    for (index, entry) in rc.profile.iter().enumerate() {
        let label = format!("profile {}", entry.name);
        let tool = artifact.blame.profile.get(index).cloned();
        let desired_when = entry
            .when
            .as_ref()
            .map(|when| serde_json::to_value(when).unwrap_or(Value::Null));
        let mut matched: Option<(String, usize)> = None;
        if let Some(previous) = prev_profile {
            for (prev_index, item) in previous.iter().enumerate() {
                if matched_prev_profile[prev_index] {
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
                    matched = Some((value, prev_index));
                    break;
                }
            }
        }
        let over = guarded_profile_over(artifact, entry, tool.as_deref());
        match matched {
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: format!("\"{}\"", entry.value),
                },
                tool,
                over: None,
            }),
            Some((previous, prev_index)) => {
                matched_prev_profile[prev_index] = true;
                if previous == entry.value {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Unchanged {
                            value: format!("\"{}\"", entry.value),
                        },
                        tool,
                        over: None,
                    });
                } else {
                    entries.push(EntryChange {
                        label,
                        change: ChangeKind::Changed {
                            from: format!("\"{previous}\""),
                            to: format!("\"{}\"", entry.value),
                        },
                        tool,
                        over,
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
        let label = format!("init[{index}]");
        match (desired_init.get(index), prev_init_text.get(index)) {
            (Some(desired), _) if empty => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: desired.clone(),
                },
                tool: artifact.blame.init.get(index).cloned(),
                over: None,
            }),
            (Some(desired), Some(Some(previous))) if desired == previous => {
                entries.push(EntryChange {
                    label,
                    change: ChangeKind::Unchanged {
                        value: desired.clone(),
                    },
                    tool: artifact.blame.init.get(index).cloned(),
                    over: None,
                })
            }
            (Some(desired), Some(Some(previous))) => entries.push(EntryChange {
                label,
                change: ChangeKind::Changed {
                    from: previous.clone(),
                    to: desired.clone(),
                },
                tool: artifact.blame.init.get(index).cloned(),
                over: None,
            }),
            (Some(desired), _) => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: desired.clone(),
                },
                tool: artifact.blame.init.get(index).cloned(),
                over: None,
            }),
            (None, Some(Some(previous))) => entries.push(EntryChange {
                label,
                change: ChangeKind::Removed {
                    value: previous.clone(),
                },
                tool: None,
                over: None,
            }),
            (None, _) => {}
        }
    }

    // Removed: previous-only aliases, env, profile (init handled index-wise).
    if !empty {
        let mut removed = Vec::new();
        if let Some(map) = prev_aliases {
            for (name, value) in map {
                if !rc.aliases.contains_key(name) {
                    removed.push(EntryChange {
                        label: format!("alias {name}"),
                        change: ChangeKind::Removed {
                            value: value.as_str().unwrap_or(&render_leaf(value)).to_string(),
                        },
                        tool: None,
                        over: None,
                    });
                }
            }
        }
        if let Some(previous) = prev_env {
            for (prev_index, item) in previous.iter().enumerate() {
                if !matched_prev_env[prev_index] {
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
                        tool: None,
                        over: None,
                    });
                }
            }
        }
        if let Some(previous) = prev_profile {
            for (prev_index, item) in previous.iter().enumerate() {
                if !matched_prev_profile[prev_index] {
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
                        tool: None,
                        over: None,
                    });
                }
            }
        }
        removed.sort_by(|left: &EntryChange, right: &EntryChange| left.label.cmp(&right.label));
        entries.extend(removed);
    }
    entries
}

/// Env `over` via shadow history: latest shadow winner differing from winner.
fn guarded_env_over(
    artifact: &Artifact,
    entry: &crate::model::EnvEntry,
    tool: Option<&str>,
) -> Option<String> {
    let winner = tool?;
    let mut over = None;
    for shadow in &artifact.shadowed.env {
        if shadow.entry.name == entry.name
            && when_eq(shadow.entry.when.as_ref(), entry.when.as_ref())
            && shadow.winner != winner
        {
            over = Some(shadow.winner.clone());
        }
    }
    over
}

/// Profile `over` via shadow history: latest shadow winner differing.
fn guarded_profile_over(
    artifact: &Artifact,
    entry: &crate::model::ProfileEntry,
    tool: Option<&str>,
) -> Option<String> {
    let winner = tool?;
    let mut over = None;
    for shadow in &artifact.shadowed.profile {
        if shadow.entry.name == entry.name
            && when_eq(shadow.entry.when.as_ref(), entry.when.as_ref())
            && shadow.winner != winner
        {
            over = Some(shadow.winner.clone());
        }
    }
    over
}

/// Detail table-kind artifacts via flattened dotted paths.
fn detail_table(
    artifact: &Artifact,
    table: &BTreeMap<String, Value>,
    snapshot: Option<&Value>,
) -> Vec<EntryChange> {
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
        let tool = artifact.blame.toml.get(path).cloned();
        match previous.get(path) {
            None => entries.push(EntryChange {
                label: path.clone(),
                change: ChangeKind::Added {
                    value: render_leaf(value),
                },
                tool,
                over: None,
            }),
            Some(prev) if prev == value => entries.push(EntryChange {
                label: path.clone(),
                change: ChangeKind::Unchanged {
                    value: render_leaf(value),
                },
                tool,
                over: None,
            }),
            Some(prev) => entries.push(EntryChange {
                label: path.clone(),
                change: ChangeKind::Changed {
                    from: render_leaf(prev),
                    to: render_leaf(value),
                },
                tool,
                over: None,
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
                    tool: None,
                    over: None,
                });
            }
        }
        entries.extend(removed);
    }
    entries
}

/// Detail template artifacts: vars by key plus a `src` line.
fn detail_template(
    artifact: &Artifact,
    src: &str,
    vars: &BTreeMap<String, Value>,
    snapshot: Option<&Value>,
) -> Vec<EntryChange> {
    let tool = last_tool(artifact);
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
                tool: tool.clone(),
                over: None,
            }),
            None => entries.push(EntryChange {
                label,
                change: ChangeKind::Added {
                    value: render_var(value),
                },
                tool: tool.clone(),
                over: None,
            }),
            Some(prev) if prev == value => entries.push(EntryChange {
                label,
                change: ChangeKind::Unchanged {
                    value: render_var(value),
                },
                tool: tool.clone(),
                over: None,
            }),
            Some(prev) => entries.push(EntryChange {
                label,
                change: ChangeKind::Changed {
                    from: render_var(prev),
                    to: render_var(value),
                },
                tool: tool.clone(),
                over: None,
            }),
        }
    }
    if empty {
        entries.push(EntryChange {
            label: "src".to_string(),
            change: ChangeKind::Added {
                value: src.to_string(),
            },
            tool: tool.clone(),
            over: None,
        });
    } else if prev_src == src {
        entries.push(EntryChange {
            label: "src".to_string(),
            change: ChangeKind::Unchanged {
                value: src.to_string(),
            },
            tool: tool.clone(),
            over: None,
        });
    } else {
        entries.push(EntryChange {
            label: "src".to_string(),
            change: ChangeKind::Changed {
                from: prev_src,
                to: src.to_string(),
            },
            tool: tool.clone(),
            over: None,
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
                tool: None,
                over: None,
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

/// Detail file/link/fetched as one hash-transition line.
fn detail_singleton(
    artifact: &Artifact,
    previous_entry: Option<&crate::store::StateEntry>,
    has_snapshot: bool,
    label: &str,
) -> Vec<EntryChange> {
    let tool = last_tool(artifact);
    let current = short_hash(&artifact.data_hash);
    match previous_entry {
        None => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Added { value: current },
            tool,
            over: None,
        }],
        Some(_) if !has_snapshot => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Added { value: current },
            tool,
            over: None,
        }],
        Some(previous) if previous.data_hash == artifact.data_hash => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Unchanged { value: current },
            tool,
            over: None,
        }],
        Some(previous) => vec![EntryChange {
            label: label.to_string(),
            change: ChangeKind::Changed {
                from: short_hash(&previous.data_hash),
                to: current,
            },
            tool,
            over: None,
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatten_matches_merge_blame_paths() {
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
        };
        assert_eq!(render_init(&eval), "eval \"$(zoxide init bash)\"");
        let cmd = InitEntry::Cmd {
            argv: vec!["task".into(), "--completion".into()],
        };
        assert_eq!(render_init(&cmd), "task --completion");
        let prev = serde_json::json!({"eval": {"argv": ["zoxide", "init", "bash"]}});
        assert_eq!(
            render_prev_init(&prev).as_deref(),
            Some("eval \"$(zoxide init bash)\"")
        );
    }

    #[test]
    fn empty_previous_yields_all_added() {
        let artifact = Artifact {
            kind: crate::model::ArtifactKind::Toml,
            path: "p".into(),
            data: ArtifactData::Toml(serde_json::from_value(serde_json::json!({"a": 1})).unwrap()),
            contributions: Vec::new(),
            shadowed: Default::default(),
            blame: Default::default(),
            data_hash: "new".into(),
        };
        let plan = Plan {
            version: 1,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "r".into(),
            profile: "p".into(),
            artifacts: vec![artifact],
            hooks: Vec::new(),
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
}
