//! Provides pure order-resolved merge rules.
//!
//! Merges an overlay (later tool) onto a base (earlier tools) in every
//! function here. Matching slot plus structurally equal guards gives last
//! wins with the loser recorded in shadowed; differing slots yield
//! co-winners kept in base-then-overlay order.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::artifact::{AliasShadow, BlameSet};
use crate::model::{
    Artifact, ArtifactData, EnvEntry, InitEntry, ProfileEntry, RcData, Shadowed, ShadowedSet,
    Table, when_eq,
};

/// Holds the fixed shadow reason for env/profile slot collisions.
const SAME_SLOT_REASON: &str = "same name, structurally equal when";
/// Holds the fixed shadow reason for repeated init entries.
const DUP_INIT_REASON: &str = "duplicate init entry";

/// Merges TOML tables while attributing each surviving leaf.
///
/// Yields the deep-merged table plus leaf blame keyed by dotted path.
/// Guarantees: overlay wins on scalar clashes and on type clashes; pairs
/// of objects merge key by key; overwritten subtree leaf paths take the
/// incoming tool while untouched base leaves keep their blame. Args:
/// `base` holds the earlier table, `overlay` holds the later one,
/// `base_blame` maps base dotted leaf paths to winner tools, `tool` names
/// the incoming tool. An empty base starts with empty blame.
///
/// Example:
/// ```rust
/// use std::collections::BTreeMap;
/// use confit::merge::merge_toml;
/// use confit::model::Table;
///
/// let base = Table::new();
/// let mut overlay = Table::new();
/// overlay.insert("k".into(), serde_json::json!(1));
/// let (merged, blame) = merge_toml(&base, &overlay, &BTreeMap::new(), "tool").unwrap();
/// assert_eq!(blame.get("k").map(String::as_str), Some("tool"));
/// ```
pub fn merge_toml(
    base: &Table,
    overlay: &Table,
    base_blame: &BTreeMap<String, String>,
    tool: &str,
) -> Result<(Table, BTreeMap<String, String>)> {
    reject_null_table(base)?;
    reject_null_table(overlay)?;
    Ok(deep_merge_tables_with_blame(
        base, overlay, base_blame, tool,
    ))
}

/// Performs the recursive deep-merge shared by table-kind artifacts.
///
/// Gives overlay wins on scalar clashes and on type clashes; pairs of
/// objects merge key by key while threading leaf blame. Args: `base` holds
/// the earlier table, `overlay` holds the later one, `base_blame` maps
/// base dotted leaf paths to winner tools, `tool` names the incoming tool.
/// Runs validation in callers; `merge_toml` wraps it with the null check,
/// while `merge_artifact` applies the null check to `Toml` alone (JSON and
/// YAML admit null).
fn deep_merge_tables_with_blame(
    base: &Table,
    overlay: &Table,
    base_blame: &BTreeMap<String, String>,
    tool: &str,
) -> (Table, BTreeMap<String, String>) {
    let mut merged = base.clone();
    let mut blame = base_blame.clone();
    for (key, overlay_value) in overlay {
        match merged.remove(key) {
            Some(base_value) => {
                let merged_value =
                    deep_merge_value_with_blame(&base_value, overlay_value, key, &mut blame, tool);
                merged.insert(key.clone(), merged_value);
            }
            None => {
                merged.insert(key.clone(), overlay_value.clone());
                insert_leaf_blame(overlay_value, key, &mut blame, tool);
            }
        }
    }
    (merged, blame)
}

/// Deep-merges two JSON values while attributing overwritten leaves.
///
/// Yields the merged value at `path`. Guarantees: object pairs merge key
/// by key with untouched leaves keeping blame; every other shape resolves
/// to the overlay value with its subtree leaves blamed on the incoming
/// tool. Args: `path` holds the dotted location of this value.
fn deep_merge_value_with_blame(
    base: &Value,
    overlay: &Value,
    path: &str,
    blame: &mut BTreeMap<String, String>,
    tool: &str,
) -> Value {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut merged = base_map.clone();
            for (key, overlay_value) in overlay_map {
                let child = format!("{path}.{key}");
                match merged.remove(key) {
                    Some(base_value) => {
                        let next = deep_merge_value_with_blame(
                            &base_value,
                            overlay_value,
                            &child,
                            blame,
                            tool,
                        );
                        merged.insert(key.clone(), next);
                    }
                    None => {
                        merged.insert(key.clone(), overlay_value.clone());
                        insert_leaf_blame(overlay_value, &child, blame, tool);
                    }
                }
            }
            Value::Object(merged)
        }
        (_, overlay_value) => {
            remove_subtree_blame(blame, path);
            insert_leaf_blame(overlay_value, path, blame, tool);
            overlay_value.clone()
        }
    }
}

/// Records winner attribution for every leaf under a value.
///
/// Yields blame entries at `path` and below pointing at the incoming tool.
/// Guarantees: objects fan out by dotted key, arrays fan out by
/// `path[index]`, scalars record exactly `path`. Args: `value` holds the
/// overlay subtree, `path` its dotted location.
fn insert_leaf_blame(value: &Value, path: &str, blame: &mut BTreeMap<String, String>, tool: &str) {
    match value {
        Value::Object(map) => {
            for (key, item) in map {
                insert_leaf_blame(item, &format!("{path}.{key}"), blame, tool);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                insert_leaf_blame(item, &format!("{path}[{index}]"), blame, tool);
            }
        }
        _ => {
            blame.insert(path.to_string(), tool.to_string());
        }
    }
}

/// Drops blame entries at a path and everything below it.
///
/// Yields a blame map without `path`, `path.*`, or `path[*]` keys,
/// clearing overwritten leaves before the winner records fresh ones.
/// Args: `path` holds the dotted location being replaced.
fn remove_subtree_blame(blame: &mut BTreeMap<String, String>, path: &str) {
    let dotted = format!("{path}.");
    let indexed = format!("{path}[");
    blame.retain(|key, _| key != path && !key.starts_with(&dotted) && !key.starts_with(&indexed));
}

/// Scans a table for nulls, producing an error at the first one found.
fn reject_null_table(table: &Table) -> Result<()> {
    for (key, value) in table {
        reject_null_value(value, key)?;
    }
    Ok(())
}

/// Scans one value for nulls. Args: `path` holds the dotted location used
/// in the error message.
fn reject_null_value(value: &Value, path: &str) -> Result<()> {
    match value {
        Value::Null => Err(Error::Merge(format!(
            "null value at '{path}': TOML has no null"
        ))),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                reject_null_value(item, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (key, item) in map {
                reject_null_value(item, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Merges one guarded entry list (env or profile).
///
/// Yields winners plus per-entry blame aligned 1:1 with winners order.
/// Guarantees: matching name plus structurally equal `when` (`None` equals
/// `None`) collides with last wins, the loser recorded in shadowed with
/// the winner attribution; differing slots keep both entries as
/// co-winners; winners always land in base-then-overlay order with blame
/// built atomically in the same pass.
fn merge_guarded<T>(
    base: Vec<T>,
    base_blame: Vec<String>,
    overlay: Vec<T>,
    tool: &str,
    name_of: impl Fn(&T) -> &str,
    when_of: impl Fn(&T) -> Option<&crate::model::Condition>,
) -> (Vec<T>, Vec<Shadowed<T>>, Vec<String>) {
    let mut winners = base;
    let mut blame = base_blame;
    let mut shadowed = Vec::new();
    for entry in overlay {
        let collision = winners.iter().position(|winner| {
            name_of(winner) == name_of(&entry) && when_eq(when_of(winner), when_of(&entry))
        });
        if let Some(index) = collision {
            let loser = winners.remove(index);
            let _ = blame.remove(index);
            shadowed.push(Shadowed {
                by_tool: tool.to_string(),
                reason: SAME_SLOT_REASON.to_string(),
                entry: loser,
                winner: tool.to_string(),
            });
        }
        winners.push(entry);
        blame.push(tool.to_string());
    }
    (winners, shadowed, blame)
}

/// Merges env entries across tools.
///
/// Yields winners, shadowed losers, and per-entry blame aligned 1:1 with
/// winners order. Guarantees: matching name plus structurally equal `when`
/// collides with last wins and the loser recorded with winner attribution;
/// matching name with differing guards yields co-winners in
/// base-then-overlay order. Args: `base` holds earlier tools' entries,
/// `base_blame` holds one winner tool per base entry, `overlay` holds the
/// later tool's entries, `tool` names the incoming tool.
///
/// Example:
/// ```rust
/// use confit::merge::merge_env;
/// use confit::model::EnvEntry;
///
/// let entry = EnvEntry { name: "A".into(), value: "1".into(), when: None };
/// let (winners, shadowed, blame) = merge_env(vec![], vec![], vec![entry], "tool");
/// assert_eq!(blame, vec!["tool".to_string()]);
/// assert!(shadowed.is_empty());
/// ```
pub fn merge_env(
    base: Vec<EnvEntry>,
    base_blame: Vec<String>,
    overlay: Vec<EnvEntry>,
    tool: &str,
) -> (Vec<EnvEntry>, Vec<Shadowed<EnvEntry>>, Vec<String>) {
    merge_guarded(
        base,
        base_blame,
        overlay,
        tool,
        |entry| &entry.name,
        |entry| entry.when.as_ref(),
    )
}

/// Merges profile entries across tools.
///
/// Yields winners, shadowed losers, and per-entry blame aligned 1:1 with
/// winners order. Guarantees: follows [`merge_env`] with matching name
/// plus structurally equal `when` colliding (last wins, loser recorded
/// with winner attribution) and differing guards yielding co-winners in
/// base-then-overlay order. Args: `base` holds earlier tools' entries,
/// `base_blame` holds one winner tool per base entry, `overlay` holds the
/// later tool's entries, `tool` names the incoming tool.
pub fn merge_profile(
    base: Vec<ProfileEntry>,
    base_blame: Vec<String>,
    overlay: Vec<ProfileEntry>,
    tool: &str,
) -> (Vec<ProfileEntry>, Vec<Shadowed<ProfileEntry>>, Vec<String>) {
    merge_guarded(
        base,
        base_blame,
        overlay,
        tool,
        |entry| &entry.name,
        |entry| entry.when.as_ref(),
    )
}

/// Merges alias maps key by key.
///
/// Yields the merged map, per-key blame, and overwrite records.
/// Guarantees: matching names always collide structurally with overlay
/// winning; a differing-value overwrite records one [`AliasShadow`] with
/// winner attribution while a same-value overwrite updates blame silently
/// with no record. Args: `base` holds earlier tools' aliases,
/// `base_blame` maps base keys to winner tools, `overlay` holds the later
/// tool's aliases, `tool` names the incoming tool.
///
/// Example:
/// ```rust
/// use std::collections::BTreeMap;
/// use confit::merge::merge_aliases;
///
/// let base: BTreeMap<String, String> = [("ls".to_string(), "a".to_string())].into_iter().collect();
/// let overlay: BTreeMap<String, String> = [("ls".to_string(), "b".to_string())].into_iter().collect();
/// let (merged, blame, shadows) = merge_aliases(&base, &BTreeMap::new(), &overlay, "tool");
/// assert_eq!(merged.get("ls").map(String::as_str), Some("b"));
/// assert_eq!(shadows.len(), 1);
/// ```
pub fn merge_aliases(
    base: &BTreeMap<String, String>,
    base_blame: &BTreeMap<String, String>,
    overlay: &BTreeMap<String, String>,
    tool: &str,
) -> (
    BTreeMap<String, String>,
    BTreeMap<String, String>,
    Vec<AliasShadow>,
) {
    let mut merged = base.clone();
    let mut blame = base_blame.clone();
    let mut shadows = Vec::new();
    for (name, value) in overlay {
        match merged.get(name) {
            Some(old) if old == value => {
                merged.insert(name.clone(), value.clone());
                blame.insert(name.clone(), tool.to_string());
            }
            Some(old) => {
                let old_value = old.clone();
                merged.insert(name.clone(), value.clone());
                blame.insert(name.clone(), tool.to_string());
                shadows.push(AliasShadow {
                    name: name.clone(),
                    old_value,
                    by_tool: tool.to_string(),
                    winner: tool.to_string(),
                });
            }
            None => {
                merged.insert(name.clone(), value.clone());
                blame.insert(name.clone(), tool.to_string());
            }
        }
    }
    (merged, blame, shadows)
}

/// Merges init entries across tools.
///
/// Yields winners, shadowed duplicates, and per-entry blame aligned 1:1
/// with winners order. Guarantees: a structurally identical entry (`==` on
/// the enum) collapses to one with the duplicate recorded in shadowed with
/// winner attribution; differing entries all run in base-then-overlay
/// order. Args: `base` holds earlier tools' entries, `base_blame` holds
/// one winner tool per base entry, `overlay` holds the later tool's
/// entries, `tool` names the incoming tool.
pub fn merge_init(
    base: Vec<InitEntry>,
    base_blame: Vec<String>,
    overlay: Vec<InitEntry>,
    tool: &str,
) -> (Vec<InitEntry>, Vec<Shadowed<InitEntry>>, Vec<String>) {
    let mut winners = base;
    let mut blame = base_blame;
    let mut shadowed = Vec::new();
    for entry in overlay {
        if winners.contains(&entry) {
            shadowed.push(Shadowed {
                by_tool: tool.to_string(),
                reason: DUP_INIT_REASON.to_string(),
                entry,
                winner: tool.to_string(),
            });
        } else {
            winners.push(entry);
            blame.push(tool.to_string());
        }
    }
    (winners, shadowed, blame)
}

/// Merges two per-shell rc data objects.
///
/// Yields the merged data plus the combined shadowed and blame sets.
/// Guarantees: composes [`merge_env`], [`merge_profile`],
/// [`merge_aliases`], and [`merge_init`]; blame vectors align 1:1 with
/// winners order, built atomically in the same merge pass and serialized
/// together so they cannot diverge. Args: `base` holds earlier tools'
/// data, `base_blame` holds the accumulated blame, `overlay` holds the
/// later tool's data, `tool` names the incoming tool.
pub fn merge_rc(
    base: &RcData,
    base_blame: &BlameSet,
    overlay: &RcData,
    tool: &str,
) -> Result<(RcData, ShadowedSet, BlameSet)> {
    let (env, env_shadowed, env_blame) = merge_env(
        base.env.clone(),
        base_blame.env.clone(),
        overlay.env.clone(),
        tool,
    );
    let (profile, profile_shadowed, profile_blame) = merge_profile(
        base.profile.clone(),
        base_blame.profile.clone(),
        overlay.profile.clone(),
        tool,
    );
    let (aliases, aliases_blame, alias_shadowed) =
        merge_aliases(&base.aliases, &base_blame.aliases, &overlay.aliases, tool);
    let (init, init_shadowed, init_blame) = merge_init(
        base.init.clone(),
        base_blame.init.clone(),
        overlay.init.clone(),
        tool,
    );
    Ok((
        RcData {
            profile,
            env,
            aliases,
            init,
        },
        ShadowedSet {
            env: env_shadowed,
            profile: profile_shadowed,
            init: init_shadowed,
            aliases: alias_shadowed,
        },
        BlameSet {
            aliases: aliases_blame,
            env: env_blame,
            profile: profile_blame,
            init: init_blame,
            toml: BTreeMap::new(),
        },
    ))
}

/// Merges a new contribution into an existing artifact.
///
/// Yields one artifact covering both inputs with per-entry blame wired
/// through. Guarantees: matching path with differing kind rejects with an
/// [`Error::Merge`] naming the path and both kinds; tables deep-merge
/// (`Toml` rejects nulls, `Json`/`Yaml` share the deep-merge with the null
/// check scoped to `Toml`, since JSON and YAML admit null) with leaf blame
/// threaded through; `Template`/`File`/`Link`/`Fetched` resolve to last
/// wins with empty blame; `Rc` merges with [`merge_rc`], blamed on the new
/// artifact's last contribution tool (`"unknown"` for artifacts carrying
/// an empty contribution list).
///
/// Contributions concatenate in merge order; shadowed accumulates
/// existing entries, then entries produced by this merge, then the new
/// artifact's own shadowed entries; blame flows from the existing artifact
/// through the merge with overlay entries blamed on the incoming tool.
/// `data_hash` resets to empty: the service re-hashes merged bytes,
/// keeping stale and fresh hashes distinct. Args: `existing` holds the
/// accumulated artifact, `new` holds the incoming contribution (matching
/// merge key).
pub fn merge_artifact(existing: Artifact, new: Artifact) -> Result<Artifact> {
    if existing.kind != new.kind {
        return Err(Error::Merge(format!(
            "cannot merge artifact at '{}': kind mismatch ({} vs {})",
            existing.path, existing.kind, new.kind
        )));
    }

    let tool = new
        .contributions
        .last()
        .map_or("unknown".to_string(), |contribution| {
            contribution.tool.clone()
        });
    let mut produced = ShadowedSet::default();
    let mut produced_blame = BlameSet::default();
    let data = match (&existing.data, &new.data) {
        (ArtifactData::Toml(base), ArtifactData::Toml(overlay)) => {
            let (merged, blame) = merge_toml(base, overlay, &existing.blame.toml, &tool)?;
            produced_blame.toml = blame;
            ArtifactData::Toml(merged)
        }
        (ArtifactData::Json(base), ArtifactData::Json(overlay)) => {
            let (merged, blame) =
                deep_merge_tables_with_blame(base, overlay, &existing.blame.toml, &tool);
            produced_blame.toml = blame;
            ArtifactData::Json(merged)
        }
        (ArtifactData::Yaml(base), ArtifactData::Yaml(overlay)) => {
            let (merged, blame) =
                deep_merge_tables_with_blame(base, overlay, &existing.blame.toml, &tool);
            produced_blame.toml = blame;
            ArtifactData::Yaml(merged)
        }
        (ArtifactData::Rc(base), ArtifactData::Rc(overlay)) => {
            let (merged, shadowed, blame) = merge_rc(base, &existing.blame, overlay, &tool)?;
            produced = shadowed;
            produced_blame = blame;
            ArtifactData::Rc(merged)
        }
        (ArtifactData::Template { .. }, ArtifactData::Template { .. })
        | (ArtifactData::File { .. }, ArtifactData::File { .. })
        | (ArtifactData::Link { .. }, ArtifactData::Link { .. })
        | (ArtifactData::Fetched { .. }, ArtifactData::Fetched { .. }) => new.data.clone(),
        _ => {
            return Err(Error::Merge(format!(
                "cannot merge artifact at '{}': data does not match kind {}",
                existing.path, existing.kind
            )));
        }
    };

    let mut contributions = existing.contributions;
    contributions.extend(new.contributions);
    let mut shadowed = existing.shadowed;
    shadowed.env.extend(produced.env);
    shadowed.profile.extend(produced.profile);
    shadowed.init.extend(produced.init);
    shadowed.aliases.extend(produced.aliases);
    shadowed.env.extend(new.shadowed.env);
    shadowed.profile.extend(new.shadowed.profile);
    shadowed.init.extend(new.shadowed.init);
    shadowed.aliases.extend(new.shadowed.aliases);

    Ok(Artifact {
        kind: existing.kind,
        path: existing.path,
        data,
        contributions,
        shadowed,
        blame: produced_blame,
        data_hash: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtifactKind, Condition, Contribution};

    fn warp() -> Condition {
        Condition::EnvEq {
            key: "TERM_PROGRAM".into(),
            value: "WarpTerminal".into(),
        }
    }

    fn ssh() -> Condition {
        Condition::EnvSet {
            key: "SSH_TTY".into(),
        }
    }

    fn env(name: &str, value: &str, when: Option<Condition>) -> EnvEntry {
        EnvEntry {
            name: name.into(),
            value: value.into(),
            when,
        }
    }

    fn values(entries: &[EnvEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.value.as_str()).collect()
    }

    #[test]
    fn zo_doctor_warp_ssh_trace() {
        // zoxide:env("_ZO_DOCTOR", "1") with when = nil.
        let (winners, shadowed, blame) =
            merge_env(vec![], vec![], vec![env("_ZO_DOCTOR", "1", None)], "zoxide");
        assert_eq!(values(&winners), vec!["1"]);
        assert_eq!(blame, vec!["zoxide".to_string()]);
        assert!(shadowed.is_empty());

        // ...:env("_ZO_DOCTOR", "0", { when = warp }) co-wins with nil.
        let (winners, shadowed, blame) = merge_env(
            winners,
            blame,
            vec![env("_ZO_DOCTOR", "0", Some(warp()))],
            "zoxide",
        );
        assert_eq!(values(&winners), vec!["1", "0"]);
        assert_eq!(blame, vec!["zoxide".to_string(), "zoxide".to_string()]);
        assert!(shadowed.is_empty());

        // ...:env("_ZO_DOCTOR", "2", { when = warp }) shadows "0".
        let (winners, shadowed, blame) = merge_env(
            winners,
            blame,
            vec![env("_ZO_DOCTOR", "2", Some(warp()))],
            "zoxide",
        );
        assert_eq!(values(&winners), vec!["1", "2"]);
        assert_eq!(blame, vec!["zoxide".to_string(), "zoxide".to_string()]);
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].entry.value, "0");
        assert_eq!(shadowed[0].by_tool, "zoxide");
        assert_eq!(shadowed[0].winner, "zoxide");
        assert_eq!(shadowed[0].reason, SAME_SLOT_REASON);

        // other:env("_ZO_DOCTOR", "3", { when = ssh }) co-wins.
        let (winners, shadowed, blame) = merge_env(
            winners,
            blame,
            vec![env("_ZO_DOCTOR", "3", Some(ssh()))],
            "other",
        );
        assert_eq!(values(&winners), vec!["1", "2", "3"]);
        assert_eq!(
            blame,
            vec![
                "zoxide".to_string(),
                "zoxide".to_string(),
                "other".to_string()
            ]
        );
        assert!(shadowed.is_empty());
    }

    #[test]
    fn shadowed_winner_attribution_across_tools() {
        let (winners, _, blame) = merge_env(
            vec![],
            vec![],
            vec![env("_ZO_DOCTOR", "0", Some(warp()))],
            "first",
        );
        let (winners, shadowed, blame) = merge_env(
            winners,
            blame,
            vec![env("_ZO_DOCTOR", "2", Some(warp()))],
            "second",
        );
        assert_eq!(values(&winners), vec!["2"]);
        assert_eq!(blame, vec!["second".to_string()]);
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].entry.value, "0");
        assert_eq!(shadowed[0].winner, "second");
        assert_eq!(shadowed[0].by_tool, "second");
    }

    #[test]
    fn env_co_winner_blame_alignment() {
        let (winners, _, blame) = merge_env(
            vec![env("A", "1", None)],
            vec!["base".to_string()],
            vec![env("A", "2", Some(warp())), env("B", "3", None)],
            "overlay-tool",
        );
        assert_eq!(winners.len(), 3);
        assert_eq!(
            blame,
            vec![
                "base".to_string(),
                "overlay-tool".to_string(),
                "overlay-tool".to_string()
            ]
        );
    }

    #[test]
    fn env_shadow_needs_both_name_and_guard() {
        let base = vec![env("A", "1", None), env("B", "1", Some(warp()))];
        let base_blame = vec!["base".to_string(), "base".to_string()];
        let overlay = vec![
            env("A", "2", Some(warp())),
            env("B", "2", Some(ssh())),
            env("C", "3", None),
        ];
        let (winners, shadowed, blame) = merge_env(base, base_blame, overlay, "tool");
        // All names collide textually, but every guard differs: co-winners.
        assert_eq!(winners.len(), 5);
        assert_eq!(blame.len(), 5);
        assert!(shadowed.is_empty());
    }

    #[test]
    fn profile_merges_like_env() {
        let entry = |value: &str, when: Option<Condition>| ProfileEntry {
            name: "PATH".into(),
            value: value.into(),
            op: crate::model::PathOp::Prepend,
            when,
        };
        let (winners, shadowed, blame) = merge_profile(
            vec![entry("/a", None)],
            vec!["base".to_string()],
            vec![entry("/b", None), entry("/c", Some(warp()))],
            "tool",
        );
        let kept: Vec<&str> = winners.iter().map(|e| e.value.as_str()).collect();
        assert_eq!(kept, vec!["/b", "/c"]);
        assert_eq!(blame, vec!["tool".to_string(), "tool".to_string()]);
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].entry.value, "/a");
        assert_eq!(shadowed[0].winner, "tool");
        assert_eq!(shadowed[0].reason, SAME_SLOT_REASON);
    }

    #[test]
    fn aliases_last_wins_per_key() {
        let base: BTreeMap<String, String> = [("ls", "a"), ("cat", "bat")]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let base_blame: BTreeMap<String, String> = [("ls", "a-tool"), ("cat", "a-tool")]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let overlay: BTreeMap<String, String> = [("ls", "b")]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let (merged, blame, shadows) = merge_aliases(&base, &base_blame, &overlay, "b-tool");
        assert_eq!(merged.get("ls").map(String::as_str), Some("b"));
        assert_eq!(merged.get("cat").map(String::as_str), Some("bat"));
        assert_eq!(blame.get("ls").map(String::as_str), Some("b-tool"));
        assert_eq!(blame.get("cat").map(String::as_str), Some("a-tool"));
        assert_eq!(shadows.len(), 1);
        assert_eq!(shadows[0].name, "ls");
        assert_eq!(shadows[0].old_value, "a");
        assert_eq!(shadows[0].winner, "b-tool");
    }

    #[test]
    fn alias_same_value_overwrite_is_silent() {
        let base: BTreeMap<String, String> = [("ls", "a")]
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        let (merged, blame, shadows) =
            merge_aliases(&base, &BTreeMap::new(), &base.clone(), "b-tool");
        assert_eq!(merged.get("ls").map(String::as_str), Some("a"));
        assert_eq!(blame.get("ls").map(String::as_str), Some("b-tool"));
        assert!(shadows.is_empty());
    }

    #[test]
    fn init_dedups_identical_entries() {
        let eval = InitEntry::Eval {
            argv: vec!["zoxide".into(), "init".into(), "bash".into()],
        };
        let cmd = InitEntry::Cmd {
            argv: vec!["task".into()],
        };
        let (winners, shadowed, blame) = merge_init(
            vec![eval.clone()],
            vec!["base".to_string()],
            vec![eval.clone(), cmd.clone()],
            "tool",
        );
        assert_eq!(winners, vec![eval, cmd]);
        assert_eq!(blame, vec!["base".to_string(), "tool".to_string()]);
        assert_eq!(shadowed.len(), 1);
        assert_eq!(shadowed[0].reason, DUP_INIT_REASON);
        assert_eq!(shadowed[0].by_tool, "tool");
        assert_eq!(shadowed[0].winner, "tool");
    }

    #[test]
    fn toml_deep_merges_and_overlay_wins_clashes() {
        let base: Table = serde_json::from_value(serde_json::json!({
            "keep": 1,
            "nested": {"a": 1, "b": 1},
            "clash": {"x": 1},
            "scalar": "base",
        }))
        .unwrap();
        let overlay: Table = serde_json::from_value(serde_json::json!({
            "nested": {"b": 2, "c": 3},
            "clash": "flat",
            "scalar": "overlay",
            "added": true,
        }))
        .unwrap();
        let (merged, _) = merge_toml(&base, &overlay, &BTreeMap::new(), "tool").unwrap();
        let expected: Table = serde_json::from_value(serde_json::json!({
            "keep": 1,
            "nested": {"a": 1, "b": 2, "c": 3},
            "clash": "flat",
            "scalar": "overlay",
            "added": true,
        }))
        .unwrap();
        assert_eq!(merged, expected);
    }

    #[test]
    fn toml_leaf_blame_through_overwrite() {
        let base: Table =
            serde_json::from_value(serde_json::json!({"keep": 1, "nested": {"a": 1, "b": 1}}))
                .unwrap();
        let mut base_blame = BTreeMap::new();
        base_blame.insert("keep".to_string(), "base-tool".to_string());
        base_blame.insert("nested.a".to_string(), "base-tool".to_string());
        base_blame.insert("nested.b".to_string(), "base-tool".to_string());
        let overlay: Table =
            serde_json::from_value(serde_json::json!({"nested": {"b": 2, "c": 3}})).unwrap();
        let (merged, blame) = merge_toml(&base, &overlay, &base_blame, "overlay-tool").unwrap();
        let expected: Table = serde_json::from_value(
            serde_json::json!({"keep": 1, "nested": {"a": 1, "b": 2, "c": 3}}),
        )
        .unwrap();
        assert_eq!(merged, expected);
        assert_eq!(blame.get("keep").map(String::as_str), Some("base-tool"));
        assert_eq!(blame.get("nested.a").map(String::as_str), Some("base-tool"));
        assert_eq!(
            blame.get("nested.b").map(String::as_str),
            Some("overlay-tool")
        );
        assert_eq!(
            blame.get("nested.c").map(String::as_str),
            Some("overlay-tool")
        );
    }

    #[test]
    fn toml_rejects_null_on_either_side() {
        let clean: Table = serde_json::from_value(serde_json::json!({"a": 1})).unwrap();
        let null_top: Table = serde_json::from_value(serde_json::json!({"a": null})).unwrap();
        let null_nested: Table =
            serde_json::from_value(serde_json::json!({"a": {"b": [1, null]}})).unwrap();
        let empty = BTreeMap::new();
        assert!(matches!(
            merge_toml(&clean, &null_top, &empty, "tool"),
            Err(Error::Merge(_))
        ));
        assert!(matches!(
            merge_toml(&null_top, &clean, &empty, "tool"),
            Err(Error::Merge(_))
        ));
        assert!(matches!(
            merge_toml(&clean, &null_nested, &empty, "tool"),
            Err(Error::Merge(_))
        ));
        assert!(merge_toml(&clean, &clean, &empty, "tool").is_ok());
    }

    fn artifact(kind: ArtifactKind, data: ArtifactData, tool: &str) -> Artifact {
        Artifact {
            kind,
            path: "p".into(),
            data,
            contributions: vec![Contribution {
                tool: tool.into(),
                order: 0,
            }],
            shadowed: ShadowedSet::default(),
            blame: BlameSet::default(),
            data_hash: String::new(),
        }
    }

    #[test]
    fn artifact_kind_mismatch_names_path_and_kinds() {
        let existing = artifact(ArtifactKind::Toml, ArtifactData::Toml(Table::new()), "a");
        let new = artifact(ArtifactKind::Json, ArtifactData::Json(Table::new()), "b");
        let error = merge_artifact(existing, new).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("'p'"), "{message}");
        assert!(message.contains("toml"), "{message}");
        assert!(message.contains("json"), "{message}");
    }

    #[test]
    fn artifact_merge_concatenates_blame_and_resets_hash() {
        let mut existing = artifact(
            ArtifactKind::File,
            ArtifactData::File {
                content: "old".into(),
            },
            "a",
        );
        existing.data_hash = "stale".into();
        let new = artifact(
            ArtifactKind::File,
            ArtifactData::File {
                content: "new".into(),
            },
            "b",
        );
        let merged = merge_artifact(existing, new).unwrap();
        assert_eq!(
            merged.data,
            ArtifactData::File {
                content: "new".into()
            }
        );
        let tools: Vec<&str> = merged
            .contributions
            .iter()
            .map(|contribution| contribution.tool.as_str())
            .collect();
        assert_eq!(tools, vec!["a", "b"]);
        assert_eq!(merged.data_hash, "");
    }
}
