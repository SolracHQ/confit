//! Merge
//!
//! Pure order-resolved merge rules.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::state::artifact::Artifact;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::artifact::Table;
use crate::model::state::condition::when_eq;
use crate::model::state::rc::AliasEntry;
use crate::model::state::rc::EnvEntry;
use crate::model::state::rc::InitEntry;
use crate::model::state::rc::ProfileEntry;
use crate::model::state::rc::RcData;

/// Leaf winner pairs keyed by dotted path, never stored.
///
/// Maps each structured leaf to its winning config plus carried priority.
/// Threads through table merges as a merge-time local; callers drop it after the fold.
pub type LeafWinners = BTreeMap<String, (String, u32)>;

/// Reports whether an overlay challenger beats a base winner.
///
/// Same-config collisions keep declaration order with the overlay winning silently.
/// Cross-config collisions resolve by highest carried priority, with ties broken by the
/// lexicographically smaller config name, so identical input sets in different
/// registration orders share one winner.
///
/// # Arguments
///
/// * `base_tool` - the current winner config.
/// * `base_priority` - the current winner priority.
/// * `overlay_tool` - the incoming challenger config.
/// * `overlay_priority` - the incoming challenger priority.
///
/// # Returns
///
/// True while the challenger takes the slot.
///
/// # Examples
/// ```rust
/// use confit::services::merge::overlay_wins;
///
/// assert!(overlay_wins("eza", 0, "bat", 0));
/// assert!(!overlay_wins("bat", 0, "eza", 0));
/// assert!(overlay_wins("bat", 0, "eza", 1));
/// assert!(!overlay_wins("eza", 1, "bat", 0));
/// ```
pub fn overlay_wins(
    base_tool: &str,
    base_priority: u32,
    overlay_tool: &str,
    overlay_priority: u32,
) -> bool {
    if base_tool == overlay_tool {
        return true;
    }
    overlay_priority > base_priority
        || (overlay_priority == base_priority && overlay_tool < base_tool)
}

/// Picks the winning config from candidates carrying priorities.
///
/// # Arguments
///
/// * `candidates` - competing configs with carried priorities, holding at least one entry.
///
/// # Returns
///
/// The winner: highest priority, ties broken by the lexicographically smaller name.
/// Yields `None` for empty candidate sets.
fn pick_winner<'a>(candidates: &[(&'a str, u32)]) -> Option<&'a str> {
    candidates
        .iter()
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(left.0)))
        .map(|(name, _)| *name)
}

/// Merges TOML tables with deterministic winner resolution.
///
/// Same-leaf collisions resolve by highest carried priority with ties broken by the
/// lexicographically smaller config name; every cross-config collision records one log line.
/// Winner pairs thread through as merge-time locals so successive folds resolve
/// against the true base winner; callers drop the map after the fold.
///
/// # Arguments
///
/// * `base` - the earlier table.
/// * `overlay` - the later table.
/// * `base_winners` - base dotted leaf paths to winner pairs.
/// * `tool` - names the incoming config.
/// * `priority` - merge priority carried by the overlay artifact.
///
/// # Returns
///
/// Merged table plus updated leaf winners keyed by dotted path.
///
/// # Errors
///
/// Null values on either side yield merge errors.
///
/// # Examples
/// ```rust
/// use confit::services::merge::merge_toml;
/// use confit::model::state::artifact::Table;
///
/// let base = Table::new();
/// let mut overlay = Table::new();
/// overlay.insert("k".into(), serde_json::json!(1));
/// let (merged, winners) = match merge_toml(&base, &overlay, &Default::default(), "tool", 0) {
///     Ok(pair) => pair,
///     Err(error) => panic!("merge succeeds: {error}"),
/// };
/// assert_eq!(winners.get("k").map(|(tool, _)| tool.as_str()), Some("tool"));
/// assert!(merged.contains_key("k"));
/// ```
pub fn merge_toml(
    base: &Table,
    overlay: &Table,
    base_winners: &LeafWinners,
    tool: &str,
    priority: u32,
) -> Result<(Table, LeafWinners)> {
    reject_null_table(base)?;
    reject_null_table(overlay)?;
    Ok(deep_merge_tables(
        base,
        overlay,
        base_winners,
        tool,
        priority,
        "toml",
    ))
}

/// Collects the distinct base winner pairs at a path and below.
///
/// # Arguments
///
/// * `winners` - dotted leaf paths to winner pairs.
/// * `path` - the dotted location under inspection.
///
/// # Returns
///
/// Sorted distinct winner pairs covering the path.
fn winners_under(winners: &LeafWinners, path: &str) -> Vec<(String, u32)> {
    let dotted = format!("{path}.");
    let indexed = format!("{path}[");
    let mut pairs: Vec<(String, u32)> = winners
        .iter()
        .filter(|(key, _)| *key == path || key.starts_with(&dotted) || key.starts_with(&indexed))
        .map(|(_, pair)| pair.clone())
        .collect();
    pairs.sort();
    pairs.dedup();
    pairs
}

/// Resolves one leaf clash between base winners and an overlay challenger.
///
/// # Arguments
///
/// * `kind` - the slot kind label for the log line.
/// * `path` - the dotted location of the clash.
/// * `base_pairs` - distinct base winner pairs covering the path.
/// * `overlay_tool` - names the incoming tool.
/// * `overlay_priority` - priority carried by the overlay artifact.
///
/// # Returns
///
/// The winning tool name.
fn resolve_leaf(
    kind: &str,
    path: &str,
    base_pairs: &[(String, u32)],
    overlay_tool: &str,
    overlay_priority: u32,
) -> String {
    let cross = base_pairs
        .iter()
        .any(|(base_tool, _)| base_tool != overlay_tool);
    if !cross {
        return overlay_tool.to_string();
    }
    let mut candidates: Vec<(&str, u32)> = base_pairs
        .iter()
        .map(|(tool, priority)| (tool.as_str(), *priority))
        .collect();
    candidates.push((overlay_tool, overlay_priority));
    let winner = pick_winner(&candidates).unwrap_or(overlay_tool);
    if winner == overlay_tool {
        let rivals: Vec<(&str, u32)> = candidates
            .into_iter()
            .filter(|(base_tool, _)| *base_tool != overlay_tool)
            .collect();
        let loser = pick_winner(&rivals).unwrap_or(overlay_tool);
        log::debug!(
            "collision on {kind} \"{path}\": \"{loser}\" overwritten, \"{overlay_tool}\" wins"
        );
    } else {
        log::debug!(
            "collision on {kind} \"{path}\": \"{overlay_tool}\" overwritten, \"{winner}\" wins"
        );
    }
    winner.to_string()
}

/// Merges tables with deterministic winner resolution.
///
/// # Arguments
///
/// * `base` - the earlier table.
/// * `overlay` - the later table.
/// * `base_winners` - base dotted leaf paths to winner pairs.
/// * `tool` - names the incoming config.
/// * `priority` - merge priority carried by the overlay artifact.
/// * `kind` - the slot kind label for log lines.
///
/// # Returns
///
/// Merged table plus updated leaf winners keyed by dotted path.
fn deep_merge_tables(
    base: &Table,
    overlay: &Table,
    base_winners: &LeafWinners,
    tool: &str,
    priority: u32,
    kind: &str,
) -> (Table, LeafWinners) {
    let mut merged = base.clone();
    let mut winners = base_winners.clone();
    for (key, overlay_value) in overlay {
        match merged.remove(key) {
            Some(base_value) => {
                let mut ctx = DeepCtx {
                    winners: &mut winners,
                    tool,
                    priority,
                    kind,
                };
                let merged_value = deep_merge_value(&base_value, overlay_value, key, &mut ctx);
                merged.insert(key.clone(), merged_value);
            }
            None => {
                merged.insert(key.clone(), overlay_value.clone());
                insert_leaf_winner(overlay_value, key, &mut winners, tool, priority);
            }
        }
    }
    (merged, winners)
}

/// Deep merge context for structured values.
///
/// Bundles the winner side so recursive merges stay under the argument limit.
///
/// * `winners` - the winner map under update.
/// * `tool` - names the incoming config.
/// * `priority` - merge priority carried by the overlay artifact.
/// * `kind` - the slot kind label for log lines.
struct DeepCtx<'a> {
    winners: &'a mut LeafWinners,
    tool: &'a str,
    priority: u32,
    kind: &'a str,
}

/// Merges two JSON values with deterministic winner resolution.
///
/// # Arguments
///
/// * `base` - the earlier value.
/// * `overlay` - the later value.
/// * `path` - the dotted location of this value.
/// * `ctx` - the deep merge context.
///
/// # Returns
///
/// Merged value at the given path.
fn deep_merge_value(base: &Value, overlay: &Value, path: &str, ctx: &mut DeepCtx<'_>) -> Value {
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            let mut merged = base_map.clone();
            for (key, overlay_value) in overlay_map {
                let child = format!("{path}.{key}");
                match merged.remove(key) {
                    Some(base_value) => {
                        let next = deep_merge_value(&base_value, overlay_value, &child, ctx);
                        merged.insert(key.clone(), next);
                    }
                    None => {
                        merged.insert(key.clone(), overlay_value.clone());
                        insert_leaf_winner(
                            overlay_value,
                            &child,
                            ctx.winners,
                            ctx.tool,
                            ctx.priority,
                        );
                    }
                }
            }
            Value::Object(merged)
        }
        (_, overlay_value) => {
            let base_pairs = winners_under(ctx.winners, path);
            let winner = resolve_leaf(ctx.kind, path, &base_pairs, ctx.tool, ctx.priority);
            if winner == ctx.tool {
                remove_subtree_winners(ctx.winners, path);
                insert_leaf_winner(overlay_value, path, ctx.winners, ctx.tool, ctx.priority);
                overlay_value.clone()
            } else {
                base.clone()
            }
        }
    }
}

/// Visits every leaf under a value in dotted path order.
///
/// # Arguments
///
/// * `value` - the subtree under walk.
/// * `path` - the dotted location of the subtree.
/// * `visit` - the callback for each leaf path plus leaf value.
pub(crate) fn walk_leaves(value: &Value, path: &str, visit: &mut impl FnMut(&str, &Value)) {
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

/// Records winner pairs for leaves under a value.
///
/// # Arguments
///
/// * `value` - the overlay subtree.
/// * `path` - the dotted location of the subtree.
/// * `winners` - the winner map under update.
/// * `tool` - names the incoming config.
/// * `priority` - merge priority carried by the overlay artifact.
fn insert_leaf_winner(
    value: &Value,
    path: &str,
    winners: &mut LeafWinners,
    tool: &str,
    priority: u32,
) {
    walk_leaves(value, path, &mut |leaf_path, _| {
        winners.insert(leaf_path.to_string(), (tool.to_string(), priority));
    });
}

/// Clears winner entries at a path and below.
///
/// # Arguments
///
/// * `winners` - the winner map under update.
/// * `path` - the dotted location under replacement.
fn remove_subtree_winners(winners: &mut LeafWinners, path: &str) {
    let dotted = format!("{path}.");
    let indexed = format!("{path}[");
    winners.retain(|key, _| key != path && !key.starts_with(&dotted) && !key.starts_with(&indexed));
}

/// Validates a table against null values.
///
/// # Arguments
///
/// * `table` - the table under validation.
///
/// # Errors
///
/// Null values yield merge errors naming the dotted path.
fn reject_null_table(table: &Table) -> Result<()> {
    for (key, value) in table {
        reject_null_value(value, key)?;
    }
    Ok(())
}

/// Validates one value against null values.
///
/// # Arguments
///
/// * `value` - the value under validation.
/// * `path` - the dotted location used in the error message.
///
/// # Errors
///
/// Null values yield merge errors naming the dotted path.
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

/// Slot context for one overlay merge pass.
///
/// Bundles the incoming side so guarded merges stay under the argument limit.
///
/// * `tool` - names the incoming tool.
/// * `kind` - the slot kind label for log lines.
struct SlotCtx<'a> {
    tool: &'a str,
    kind: &'a str,
}

/// Merges one guarded entry list.
///
/// Same-slot collisions resolve by highest carried priority with ties broken by the
/// lexicographically smaller config name; every cross-config collision records one log line.
/// Owner names thread through as merge-time locals so successive folds resolve
/// against the true base winner; callers drop the owners after the fold.
///
/// # Arguments
///
/// * `base` - the earlier entries.
/// * `base_owners` - one winner config per base entry.
/// * `overlay` - the later entries.
/// * `ctx` - the incoming slot context.
/// * `name_of` - yields the slot name.
/// * `when_of` - yields the guard condition.
/// * `priority_of` - yields the carried priority.
///
/// # Returns
///
/// Winners plus per-entry owners aligned with winners order.
fn merge_guarded<T>(
    base: Vec<T>,
    base_owners: Vec<String>,
    overlay: Vec<T>,
    ctx: SlotCtx<'_>,
    name_of: impl Fn(&T) -> &str,
    when_of: impl Fn(&T) -> Option<&crate::model::state::condition::Condition>,
    priority_of: impl Fn(&T) -> u32,
) -> (Vec<T>, Vec<String>) {
    let mut winners = base;
    let mut owners = base_owners;
    for entry in overlay {
        let collision = winners.iter().position(|winner| {
            name_of(winner) == name_of(&entry) && when_eq(when_of(winner), when_of(&entry))
        });
        if let Some(index) = collision {
            let base_tool = owners[index].clone();
            let base_priority = priority_of(&winners[index]);
            let overlay_priority = priority_of(&entry);
            if overlay_wins(&base_tool, base_priority, ctx.tool, overlay_priority) {
                let loser = winners.remove(index);
                let _ = owners.remove(index);
                if base_tool != ctx.tool {
                    log::debug!(
                        "collision on {} \"{}\": \"{base_tool}\" overwritten, \"{}\" wins",
                        ctx.kind,
                        name_of(&loser),
                        ctx.tool,
                    );
                }
                winners.push(entry);
                owners.push(ctx.tool.to_string());
            } else {
                log::debug!(
                    "collision on {} \"{}\": \"{}\" overwritten, \"{base_tool}\" wins",
                    ctx.kind,
                    name_of(&entry),
                    ctx.tool,
                );
            }
        } else {
            winners.push(entry);
            owners.push(ctx.tool.to_string());
        }
    }
    (winners, owners)
}

/// Merges env entries across configs.
///
/// Same-slot collisions resolve by highest priority with ties broken by the
/// lexicographically smaller config name; every cross-config collision records one log line.
/// Owner names thread through as merge-time locals; callers drop them after the fold.
///
/// # Arguments
///
/// * `base` - earlier configs' entries.
/// * `base_owners` - one winner config per base entry.
/// * `overlay` - the later config's entries.
/// * `tool` - names the incoming config.
/// * `priorities` - config names to merge priorities.
///
/// # Returns
///
/// Winners plus per-entry owners aligned with winners order.
///
/// # Examples
/// ```rust
/// use std::collections::BTreeMap;
/// use confit::services::merge::merge_env;
/// use confit::model::state::rc::EnvEntry;
///
/// let entry = EnvEntry { name: "A".into(), value: "1".into(), when: None, priority: 0 };
/// let (winners, owners) = merge_env(vec![], vec![], vec![entry], "tool");
/// assert_eq!(owners, vec!["tool".to_string()]);
/// assert_eq!(winners.len(), 1);
/// ```
pub fn merge_env(
    base: Vec<EnvEntry>,
    base_owners: Vec<String>,
    overlay: Vec<EnvEntry>,
    tool: &str,
) -> (Vec<EnvEntry>, Vec<String>) {
    merge_guarded(
        base,
        base_owners,
        overlay,
        SlotCtx { tool, kind: "env" },
        |entry| &entry.name,
        |entry| entry.when.as_ref(),
        |entry| entry.priority,
    )
}

/// Merges profile entries across configs.
///
/// Same-slot collisions resolve by highest priority with ties broken by the
/// lexicographically smaller config name; every cross-config collision records one log line.
/// Owner names thread through as merge-time locals; callers drop them after the fold.
///
/// # Arguments
///
/// * `base` - earlier configs' entries.
/// * `base_owners` - one winner config per base entry.
/// * `overlay` - the later config's entries.
/// * `tool` - names the incoming config.
///
/// # Returns
///
/// Winners plus per-entry owners aligned with winners order.
pub fn merge_profile(
    base: Vec<ProfileEntry>,
    base_owners: Vec<String>,
    overlay: Vec<ProfileEntry>,
    tool: &str,
) -> (Vec<ProfileEntry>, Vec<String>) {
    merge_guarded(
        base,
        base_owners,
        overlay,
        SlotCtx {
            tool,
            kind: "profile",
        },
        |entry| &entry.name,
        |entry| entry.when.as_ref(),
        |entry| entry.priority,
    )
}

/// Merges alias entries across configs.
///
/// Slots match by (`name`, structural `when`), exactly like env entries: entries holding
/// different guards coexist. Same-slot collisions resolve by highest priority with ties
/// broken by the lexicographically smaller config name; every cross-config collision with
/// differing values records one log line. Same-value replacements name the deterministic
/// winner silently with zero log lines. Owner names thread through as merge-time locals.
///
/// # Arguments
///
/// * `base` - earlier configs' entries.
/// * `base_owners` - one winner config per base entry.
/// * `overlay` - the later config's entries.
/// * `tool` - names the incoming config.
///
/// # Returns
///
/// Winners plus per-entry owners aligned with winners order.
///
/// # Examples
/// ```rust
/// use confit::services::merge::merge_aliases;
/// use confit::model::state::rc::AliasEntry;
///
/// let base = vec![AliasEntry { name: "ls".into(), value: "a".into(), when: None, priority: 0 }];
/// let overlay = vec![AliasEntry { name: "ls".into(), value: "b".into(), when: None, priority: 0 }];
/// let (merged, owners) = merge_aliases(base, vec!["a-tool".into()], overlay, "tool");
/// assert_eq!(merged.len(), 1);
/// assert_eq!(owners, vec!["a-tool".to_string()]);
/// ```
/// Finds the same-value slot for an alias entry.
///
/// # Arguments
///
/// * `winners` - the current winners under scan.
/// * `entry` - the incoming entry under test.
///
/// # Returns
///
/// Index of the winner holding the same name plus guard plus value.
fn alias_same_value_index(winners: &[AliasEntry], entry: &AliasEntry) -> Option<usize> {
    winners.iter().position(|winner| {
        winner.name == entry.name
            && when_eq(winner.when.as_ref(), entry.when.as_ref())
            && winner.value == entry.value
    })
}

/// Keeps the deterministic winner for a same-value alias slot silently.
///
/// # Arguments
///
/// * `winners` - the current winners under update.
/// * `owners` - the owners aligned with winners.
/// * `index` - the same-value slot index.
/// * `entry` - the incoming entry.
/// * `tool` - names the incoming config.
fn keep_alias_same_value(
    winners: &mut [AliasEntry],
    owners: &mut [String],
    index: usize,
    entry: AliasEntry,
    tool: &str,
) {
    let base_tool = owners[index].clone();
    if overlay_wins(&base_tool, winners[index].priority, tool, entry.priority) {
        winners[index] = entry;
        owners[index] = tool.to_string();
    }
}

pub fn merge_aliases(
    base: Vec<AliasEntry>,
    base_owners: Vec<String>,
    overlay: Vec<AliasEntry>,
    tool: &str,
) -> (Vec<AliasEntry>, Vec<String>) {
    let mut winners = base;
    let mut owners = base_owners;
    for entry in overlay {
        if let Some(index) = alias_same_value_index(&winners, &entry) {
            keep_alias_same_value(&mut winners, &mut owners, index, entry, tool);
            continue;
        }
        (winners, owners) = merge_guarded(
            winners,
            owners,
            vec![entry],
            SlotCtx {
                tool,
                kind: "alias",
            },
            |item| &item.name,
            |item| item.when.as_ref(),
            |item| item.priority,
        );
    }
    (winners, owners)
}

/// Merges init entries across configs.
///
/// Identity covers the spec plus the guard: entries holding different `when` guards coexist,
/// while an overlay entry identical to a winner drops as a duplicate with first-writer
/// winning, so resolution stays deterministic. Priority stays out of the identity:
/// same spec plus guard pairs dedup holding the first writer. Duplicates need no
/// owner tracking.
///
/// # Arguments
///
/// * `base` - earlier configs' entries.
/// * `overlay` - the later config's entries.
///
/// # Returns
///
/// Winners in merge order.
pub fn merge_init(base: Vec<InitEntry>, overlay: Vec<InitEntry>) -> Vec<InitEntry> {
    let mut winners = base;
    for entry in overlay {
        if !winners.iter().any(|winner| init_same(winner, &entry)) {
            winners.push(entry);
        }
    }
    winners
}

/// Reports init identity ignoring merge priority.
///
/// # Arguments
///
/// * `left` - the first entry under comparison.
/// * `right` - the second entry under comparison.
///
/// # Returns
///
/// True for matching specs plus structurally equal guards.
fn init_same(left: &InitEntry, right: &InitEntry) -> bool {
    match (left, right) {
        (
            InitEntry::Eval {
                argv: left_argv,
                when: left_when,
                ..
            },
            InitEntry::Eval {
                argv: right_argv,
                when: right_when,
                ..
            },
        ) => left_argv == right_argv && when_eq(left_when.as_ref(), right_when.as_ref()),
        (
            InitEntry::Cmd {
                argv: left_argv,
                when: left_when,
                ..
            },
            InitEntry::Cmd {
                argv: right_argv,
                when: right_when,
                ..
            },
        ) => left_argv == right_argv && when_eq(left_when.as_ref(), right_when.as_ref()),
        (
            InitEntry::Source {
                path: left_path,
                when: left_when,
                ..
            },
            InitEntry::Source {
                path: right_path,
                when: right_when,
                ..
            },
        ) => left_path == right_path && when_eq(left_when.as_ref(), right_when.as_ref()),
        _ => false,
    }
}

/// Merge-time winner owners for one rc fold, never stored.
///
/// Threads through successive `merge_rc` calls so each slot resolves against
/// the true base winner; the fold drops the whole struct once rendering starts.
/// Init needs no owners: duplicates resolve by value with first-writer winning.
#[derive(Debug, Clone, Default)]
pub struct RcWinners {
    /// Holds one winner config per env winner, aligned with winners order.
    pub env: Vec<String>,
    /// Holds one winner config per profile winner, aligned with winners order.
    pub profile: Vec<String>,
    /// Holds one winner config per alias winner, aligned with winners order.
    pub aliases: Vec<String>,
}

/// Merge-time winner state for one artifact path, never stored.
///
/// Threads through successive `merge_artifact` calls at one path so each slot
/// resolves against the true base winner; the fold drops the whole struct once
/// the artifact lands in the plan.
#[derive(Debug, Clone, Default)]
pub struct ArtifactWinners {
    /// Holds leaf winner pairs for `toml`/`json`/`yaml` tables keyed by dotted path.
    pub leaves: LeafWinners,
    /// Holds rc slot owners for `rc` data.
    pub rc: RcWinners,
    /// Holds every contributor config with carried priority for singleton kinds.
    pub contributors: Vec<(String, u32)>,
}

/// Merges two per-shell rc data objects.
///
/// Owner names thread through as merge-time locals so successive folds resolve
/// against the true base winner; callers drop the owners after the fold.
///
/// # Arguments
///
/// * `base` - earlier configs' data.
/// * `base_owners` - the accumulated owners.
/// * `overlay` - the later config's data.
/// * `tool` - names the incoming config.
///
/// # Returns
///
/// Merged data plus updated owners.
///
/// # Errors
///
/// Infallible. Yields `Ok` for all inputs.
pub fn merge_rc(
    base: &RcData,
    base_owners: &RcWinners,
    overlay: &RcData,
    tool: &str,
) -> Result<(RcData, RcWinners)> {
    let (env, env_owners) = merge_env(
        base.env.clone(),
        base_owners.env.clone(),
        overlay.env.clone(),
        tool,
    );
    let (profile, profile_owners) = merge_profile(
        base.profile.clone(),
        base_owners.profile.clone(),
        overlay.profile.clone(),
        tool,
    );
    let (aliases, alias_owners) = merge_aliases(
        base.aliases.clone(),
        base_owners.aliases.clone(),
        overlay.aliases.clone(),
        tool,
    );
    let init = merge_init(base.init.clone(), overlay.init.clone());
    Ok((
        RcData {
            profile,
            env,
            aliases,
            init,
        },
        RcWinners {
            env: env_owners,
            profile: profile_owners,
            aliases: alias_owners,
        },
    ))
}

/// Merges one incoming config into an existing artifact.
///
/// Same-slot collisions resolve by highest carried priority with ties broken by the
/// lexicographically smaller config name; every cross-config collision records one log line.
/// Winner state threads through `winners` as a merge-time local; the artifact itself
/// carries data alone.
///
/// # Arguments
///
/// * `existing` - the accumulated artifact.
/// * `incoming` - the incoming payload.
/// * `tool` - names the incoming config.
/// * `priority` - merge priority carried by the incoming artifact.
/// * `winners` - the winner state for this path, updated in place.
///
/// # Returns
///
/// Single artifact covering both inputs.
///
/// # Errors
///
/// Kind mismatches and data shape mismatches yield merge errors naming the path. TOML null
/// values yield merge errors.
pub fn merge_artifact(
    existing: Artifact,
    incoming: ArtifactData,
    tool: &str,
    priority: u32,
    winners: &mut ArtifactWinners,
) -> Result<Artifact> {
    let incoming_kind = match &incoming {
        ArtifactData::Toml(_) => ArtifactKind::Toml,
        ArtifactData::Json(_) => ArtifactKind::Json,
        ArtifactData::Yaml(_) => ArtifactKind::Yaml,
        ArtifactData::Template { .. } => ArtifactKind::Template,
        ArtifactData::File { .. } => ArtifactKind::File,
        ArtifactData::Link { .. } => ArtifactKind::Link,
        ArtifactData::Rc(_) => ArtifactKind::Rc,
    };
    if existing.kind != incoming_kind {
        return Err(Error::Merge(format!(
            "cannot merge artifact at '{}': kind mismatch ({} vs {})",
            existing.path, existing.kind, incoming_kind
        )));
    }

    let data = match (&existing.data, &incoming) {
        (ArtifactData::Toml(base), ArtifactData::Toml(overlay)) => {
            let (merged, leaves) = merge_toml(base, overlay, &winners.leaves, tool, priority)?;
            winners.leaves = leaves;
            ArtifactData::Toml(merged)
        }
        (ArtifactData::Json(base), ArtifactData::Json(overlay)) => {
            let (merged, leaves) =
                deep_merge_tables(base, overlay, &winners.leaves, tool, priority, "json");
            winners.leaves = leaves;
            ArtifactData::Json(merged)
        }
        (ArtifactData::Yaml(base), ArtifactData::Yaml(overlay)) => {
            let (merged, leaves) =
                deep_merge_tables(base, overlay, &winners.leaves, tool, priority, "yaml");
            winners.leaves = leaves;
            ArtifactData::Yaml(merged)
        }
        (ArtifactData::Rc(base), ArtifactData::Rc(overlay)) => {
            let (merged, owners) = merge_rc(base, &winners.rc, overlay, tool)?;
            winners.rc = owners;
            ArtifactData::Rc(merged)
        }
        (ArtifactData::Template { .. }, ArtifactData::Template { .. })
        | (ArtifactData::File { .. }, ArtifactData::File { .. })
        | (ArtifactData::Link { .. }, ArtifactData::Link { .. }) => {
            let candidates: Vec<(&str, u32)> = winners
                .contributors
                .iter()
                .map(|(tool, priority)| (tool.as_str(), *priority))
                .collect();
            let incumbent = pick_winner(&candidates).unwrap_or(tool);
            let incumbent_priority = candidates
                .iter()
                .find(|(name, _)| *name == incumbent)
                .map(|(_, priority)| *priority)
                .unwrap_or(0);
            if overlay_wins(incumbent, incumbent_priority, tool, priority) {
                if incumbent != tool {
                    log::debug!(
                        "collision on {} \"{}\": \"{incumbent}\" overwritten, \"{tool}\" wins",
                        existing.kind,
                        existing.path,
                    );
                }
                incoming.clone()
            } else {
                log::debug!(
                    "collision on {} \"{}\": \"{tool}\" overwritten, \"{incumbent}\" wins",
                    existing.kind,
                    existing.path,
                );
                existing.data.clone()
            }
        }
        _ => {
            return Err(Error::Merge(format!(
                "cannot merge artifact at '{}': data does not match kind {}",
                existing.path, existing.kind
            )));
        }
    };

    winners.contributors.push((tool.to_string(), priority));

    Ok(Artifact {
        kind: existing.kind,
        path: existing.path,
        data,
        data_hash: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::state::artifact::ArtifactKind;
    use crate::model::state::condition::Condition;

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
            priority: 0,
        }
    }

    fn values(entries: &[EnvEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.value.as_str()).collect()
    }

    #[test]
    fn zo_doctor_warp_ssh_trace() {
        // zoxide:env("_ZO_DOCTOR", "1") with when = nil.
        let (winners, owners) =
            merge_env(vec![], vec![], vec![env("_ZO_DOCTOR", "1", None)], "zoxide");
        assert_eq!(values(&winners), vec!["1"]);
        assert_eq!(owners, vec!["zoxide".to_string()]);

        // ...:env("_ZO_DOCTOR", "0", { when = warp }) co-wins with nil.
        let (winners, owners) = merge_env(
            winners,
            owners,
            vec![env("_ZO_DOCTOR", "0", Some(warp()))],
            "zoxide",
        );
        assert_eq!(values(&winners), vec!["1", "0"]);
        assert_eq!(owners, vec!["zoxide".to_string(), "zoxide".to_string()]);

        // ...:env("_ZO_DOCTOR", "2", { when = warp }) shadows "0".
        let (winners, owners) = merge_env(
            winners,
            owners,
            vec![env("_ZO_DOCTOR", "2", Some(warp()))],
            "zoxide",
        );
        assert_eq!(values(&winners), vec!["1", "2"]);
        assert_eq!(owners, vec!["zoxide".to_string(), "zoxide".to_string()]);

        // other:env("_ZO_DOCTOR", "3", { when = ssh }) co-wins.
        let (winners, owners) = merge_env(
            winners,
            owners,
            vec![env("_ZO_DOCTOR", "3", Some(ssh()))],
            "other",
        );
        assert_eq!(values(&winners), vec!["1", "2", "3"]);
        assert_eq!(
            owners,
            vec![
                "zoxide".to_string(),
                "zoxide".to_string(),
                "other".to_string()
            ]
        );
    }

    #[test]
    fn loser_keeps_base_winner_across_configs() {
        let (winners, owners) = merge_env(
            vec![],
            vec![],
            vec![env("_ZO_DOCTOR", "0", Some(warp()))],
            "first",
        );
        let (winners, owners) = merge_env(
            winners,
            owners,
            vec![env("_ZO_DOCTOR", "2", Some(warp()))],
            "second",
        );
        // Equal priorities resolve by lexicographically smaller config name.
        assert_eq!(values(&winners), vec!["0"]);
        assert_eq!(owners, vec!["first".to_string()]);
    }

    #[test]
    fn env_co_winner_owner_alignment() {
        let (winners, owners) = merge_env(
            vec![env("A", "1", None)],
            vec!["base".to_string()],
            vec![env("A", "2", Some(warp())), env("B", "3", None)],
            "overlay-tool",
        );
        assert_eq!(winners.len(), 3);
        assert_eq!(
            owners,
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
        let base_owners = vec!["base".to_string(), "base".to_string()];
        let overlay = vec![
            env("A", "2", Some(warp())),
            env("B", "2", Some(ssh())),
            env("C", "3", None),
        ];
        let (winners, owners) = merge_env(base, base_owners, overlay, "tool");
        // All names collide textually, but every guard differs: co-winners.
        assert_eq!(winners.len(), 5);
        assert_eq!(owners.len(), 5);
    }

    #[test]
    fn profile_merges_like_env() {
        let entry = |value: &str, when: Option<Condition>| ProfileEntry {
            name: "PATH".into(),
            value: value.into(),
            op: crate::model::state::rc::PathOp::Prepend,
            when,
            priority: 0,
        };
        let (winners, owners) = merge_profile(
            vec![entry("/a", None)],
            vec!["base".to_string()],
            vec![entry("/b", None), entry("/c", Some(warp()))],
            "tool",
        );
        // Equal priorities resolve by lexicographically smaller config name.
        let kept: Vec<&str> = winners.iter().map(|e| e.value.as_str()).collect();
        assert_eq!(kept, vec!["/a", "/c"]);
        assert_eq!(owners, vec!["base".to_string(), "tool".to_string()]);
    }

    fn alias(name: &str, value: &str, when: Option<Condition>) -> AliasEntry {
        AliasEntry {
            name: name.into(),
            value: value.into(),
            when,
            priority: 0,
        }
    }

    fn alias_values(entries: &[AliasEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.value.as_str()).collect()
    }

    #[test]
    fn aliases_resolve_by_priority_then_name() {
        let base = vec![alias("ls", "a", None), alias("cat", "bat", None)];
        let base_owners = vec!["a-tool".to_string(), "a-tool".to_string()];
        let overlay = vec![alias("ls", "b", None)];
        let (merged, owners) = merge_aliases(base, base_owners, overlay, "b-tool");
        // Equal priorities resolve by lexicographically smaller config name.
        assert_eq!(alias_values(&merged), vec!["a", "bat"]);
        assert_eq!(owners, vec!["a-tool".to_string(), "a-tool".to_string()]);
    }

    #[test]
    fn alias_higher_priority_beats_name_order() {
        let base = vec![alias("ls", "a", None)];
        let overlay = vec![AliasEntry {
            priority: 1,
            ..alias("ls", "b", None)
        }];
        let (merged, owners) = merge_aliases(base, vec!["a-tool".into()], overlay, "b-tool");
        assert_eq!(alias_values(&merged), vec!["b"]);
        assert_eq!(owners, vec!["b-tool".to_string()]);
    }

    #[test]
    fn alias_same_value_overwrite_is_silent() {
        let base = vec![alias("ls", "a", None)];
        let (merged, owners) = merge_aliases(base.clone(), vec!["a-tool".into()], base, "b-tool");
        assert_eq!(alias_values(&merged), vec!["a"]);
        assert_eq!(owners, vec!["a-tool".to_string()]);
    }

    #[test]
    fn alias_guards_resolve_by_name_and_guard() {
        let (winners, owners) =
            merge_aliases(vec![], vec![], vec![alias("cat", "bat", None)], "first");
        // Same name with a different guard co-wins.
        let (winners, owners) = merge_aliases(
            winners,
            owners,
            vec![alias("cat", "bat", Some(warp()))],
            "second",
        );
        assert_eq!(alias_values(&winners), vec!["bat", "bat"]);
        assert_eq!(owners, vec!["first".to_string(), "second".to_string()]);
        // Same name plus structurally equal guard resolves by name order.
        let (winners, owners) = merge_aliases(
            winners,
            owners,
            vec![alias("cat", "exa", Some(warp()))],
            "third",
        );
        assert_eq!(alias_values(&winners), vec!["bat", "bat"]);
        assert_eq!(owners, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn init_dedups_identical_entries() {
        let eval = InitEntry::Eval {
            argv: vec!["zoxide".into(), "init".into(), "bash".into()],
            when: None,
            priority: 0,
        };
        let cmd = InitEntry::Cmd {
            argv: vec!["task".into()],
            when: None,
            priority: 0,
        };
        let winners = merge_init(vec![eval.clone()], vec![eval.clone(), cmd.clone()]);
        assert_eq!(winners, vec![eval, cmd]);
    }

    #[test]
    fn init_guards_split_identity_by_guard() {
        let plain = InitEntry::Cmd {
            argv: vec!["task".into()],
            when: None,
            priority: 0,
        };
        let guarded = InitEntry::Cmd {
            argv: vec!["task".into()],
            when: Some(warp()),
            priority: 0,
        };
        // Same spec with a different guard coexists.
        let winners = merge_init(vec![plain.clone()], vec![guarded.clone()]);
        assert_eq!(winners, vec![plain, guarded.clone()]);
        // Identical spec plus guard drops deterministically with first-writer winning.
        let winners = merge_init(winners, vec![guarded.clone()]);
        assert_eq!(winners.len(), 2);
        assert_eq!(winners[1], guarded);
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
        // Empty base winners hold no winner, so the overlay wins silently.
        let (merged, _) = merge_toml(&base, &overlay, &BTreeMap::new(), "tool", 0).unwrap();
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
    fn toml_leaf_clash_resolves_by_name_order() {
        let base: Table =
            serde_json::from_value(serde_json::json!({"keep": 1, "nested": {"a": 1, "b": 1}}))
                .unwrap();
        let mut base_winners = BTreeMap::new();
        base_winners.insert("keep".to_string(), ("base-tool".to_string(), 0));
        base_winners.insert("nested.a".to_string(), ("base-tool".to_string(), 0));
        base_winners.insert("nested.b".to_string(), ("base-tool".to_string(), 0));
        let overlay: Table =
            serde_json::from_value(serde_json::json!({"nested": {"b": 2, "c": 3}})).unwrap();
        let (merged, winners) =
            merge_toml(&base, &overlay, &base_winners, "overlay-tool", 0).unwrap();
        // Equal priorities resolve by lexicographically smaller config name.
        let expected: Table = serde_json::from_value(
            serde_json::json!({"keep": 1, "nested": {"a": 1, "b": 1, "c": 3}}),
        )
        .unwrap();
        assert_eq!(merged, expected);
        assert_eq!(
            winners.get("keep").map(|(tool, _)| tool.as_str()),
            Some("base-tool")
        );
        assert_eq!(
            winners.get("nested.a").map(|(tool, _)| tool.as_str()),
            Some("base-tool")
        );
        assert_eq!(
            winners.get("nested.b").map(|(tool, _)| tool.as_str()),
            Some("base-tool")
        );
        assert_eq!(
            winners.get("nested.c").map(|(tool, _)| tool.as_str()),
            Some("overlay-tool")
        );
    }

    #[test]
    fn toml_rejects_null_on_either_side() {
        let clean: Table = serde_json::from_value(serde_json::json!({"a": 1})).unwrap();
        let null_top: Table = serde_json::from_value(serde_json::json!({"a": null})).unwrap();
        let null_nested: Table =
            serde_json::from_value(serde_json::json!({"a": {"b": [1, null]}})).unwrap();
        let winners = BTreeMap::new();
        assert!(matches!(
            merge_toml(&clean, &null_top, &winners, "tool", 0),
            Err(Error::Merge(_))
        ));
        assert!(matches!(
            merge_toml(&null_top, &clean, &winners, "tool", 0),
            Err(Error::Merge(_))
        ));
        assert!(matches!(
            merge_toml(&clean, &null_nested, &winners, "tool", 0),
            Err(Error::Merge(_))
        ));
        assert!(merge_toml(&clean, &clean, &winners, "tool", 0).is_ok());
    }

    fn artifact(kind: ArtifactKind, data: ArtifactData) -> Artifact {
        Artifact {
            kind,
            path: "p".into(),
            data,
            data_hash: String::new(),
        }
    }

    fn winners_for(tool: &str) -> ArtifactWinners {
        ArtifactWinners {
            contributors: vec![(tool.to_string(), 0)],
            ..ArtifactWinners::default()
        }
    }

    #[test]
    fn artifact_kind_mismatch_names_path_and_kinds() {
        let existing = artifact(ArtifactKind::Toml, ArtifactData::Toml(Table::new()));
        let incoming = ArtifactData::Json(Table::new());
        let mut winners = winners_for("a");
        let error = merge_artifact(existing, incoming, "b", 0, &mut winners).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("'p'"), "{message}");
        assert!(message.contains("toml"), "{message}");
        assert!(message.contains("json"), "{message}");
    }

    #[test]
    fn artifact_merge_resolves_winner_and_resets_hash() {
        let mut existing = artifact(
            ArtifactKind::File,
            ArtifactData::File {
                content: "old".into(),
            },
        );
        existing.data_hash = "stale".into();
        let incoming = ArtifactData::File {
            content: "new".into(),
        };
        let mut winners = winners_for("a");
        // Equal priorities resolve by lexicographically smaller config name.
        let merged = merge_artifact(existing, incoming, "b", 0, &mut winners).unwrap();
        assert_eq!(
            merged.data,
            ArtifactData::File {
                content: "old".into()
            }
        );
        assert_eq!(merged.data_hash, "");
    }

    #[test]
    fn alias_registration_order_does_not_change_winner() {
        let bat = vec![alias("cat", "bat", None)];
        let eza = vec![alias("cat", "eza", None)];
        let (forward, forward_owners) =
            merge_aliases(bat.clone(), vec!["bat".into()], eza.clone(), "eza");
        let (backward, backward_owners) = merge_aliases(eza, vec!["eza".into()], bat, "bat");
        assert_eq!(alias_values(&forward), vec!["bat"]);
        assert_eq!(forward, backward);
        assert_eq!(forward_owners, backward_owners);
        assert_eq!(forward_owners, vec!["bat".to_string()]);
    }

    #[test]
    fn env_priority_winner_ignores_registration_order() {
        let high = vec![EnvEntry {
            priority: 1,
            ..env("EDITOR", "hx", None)
        }];
        let low = vec![env("EDITOR", "vim", None)];
        let (forward, forward_owners) =
            merge_env(high.clone(), vec!["high".into()], low.clone(), "low");
        let (backward, backward_owners) = merge_env(low, vec!["low".into()], high, "high");
        assert_eq!(values(&forward), vec!["hx"]);
        assert_eq!(forward, backward);
        assert_eq!(forward_owners, backward_owners);
        assert_eq!(forward_owners, vec!["high".to_string()]);
    }

    #[test]
    fn toml_registration_order_does_not_change_winner() {
        let table_for = |value: i64| {
            serde_json::from_value(serde_json::json!({"key": value})).unwrap_or_default()
        };
        let winners_for_leaf =
            |tool: &str| BTreeMap::from([("key".to_string(), (tool.to_string(), 0))]);
        let (forward, forward_winners) = merge_toml(
            &table_for(1),
            &table_for(2),
            &winners_for_leaf("bat"),
            "eza",
            0,
        )
        .unwrap();
        let (backward, backward_winners) = merge_toml(
            &table_for(2),
            &table_for(1),
            &winners_for_leaf("eza"),
            "bat",
            0,
        )
        .unwrap();
        assert_eq!(forward, backward);
        assert_eq!(forward_winners, backward_winners);
        assert_eq!(
            forward_winners.get("key").map(|(tool, _)| tool.as_str()),
            Some("bat")
        );
    }

    #[test]
    fn file_registration_order_does_not_change_winner() {
        let file = |content: &str| {
            artifact(
                ArtifactKind::File,
                ArtifactData::File {
                    content: content.into(),
                },
            )
        };
        let forward = merge_artifact(
            file("old"),
            ArtifactData::File {
                content: "new".into(),
            },
            "b",
            0,
            &mut winners_for("a"),
        )
        .unwrap();
        let backward = merge_artifact(
            file("new"),
            ArtifactData::File {
                content: "old".into(),
            },
            "a",
            0,
            &mut winners_for("b"),
        )
        .unwrap();
        assert_eq!(forward.data, backward.data);
    }
}
