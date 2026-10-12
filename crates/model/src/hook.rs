//! Hook
//! Post-config steps follow plans beside documents.

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

use crate::arg::Arg;
use crate::condition::Condition;

/// One post-config step with conditions and checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(deny_unknown_fields)]
#[face(table, validate = reject_empty_argv)]
pub struct Hook {
    /// Command for the subprocess.
    pub argv: Vec<Arg>,
    /// PATH entries for the subprocess alone.
    #[face(default)]
    pub path: Vec<Arg>,
    /// The capability condition.
    #[serde(default)]
    pub requires: Option<Condition>,
    /// The need condition.
    pub when: Option<Condition>,
    /// AND conditions for the run.
    #[face(default)]
    pub checks: Vec<Condition>,
    /// The run cap in seconds.
    #[face(rename = "timeout", proxy = String, convert = decode_timeout, encode = render_timeout, default = default_timeout_secs)]
    pub timeout_secs: u64,
}

/// One condition slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
pub enum ConditionSlot {
    /// The capability condition.
    Requires,
    /// The need condition.
    When,
}

/// One condition change.
#[derive(Debug, Clone, PartialEq, Eq, Face)]
#[face(table)]
pub struct ConditionChange {
    /// The slot under diffing.
    pub slot: ConditionSlot,
    /// The recorded condition.
    pub before: Option<Condition>,
    /// The desired condition.
    pub after: Option<Condition>,
}

/// One timeout change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table)]
pub struct TimeoutChange {
    /// The recorded cap in seconds.
    pub before: u64,
    /// The desired cap in seconds.
    pub after: u64,
}

/// One hook modification.
#[derive(Debug, Clone, PartialEq, Eq, Face)]
#[face(table)]
pub struct HookModification {
    /// Condition edits in slot order.
    pub conditions: Vec<ConditionChange>,
    /// New checks missing from the recorded order.
    pub added_checks: Vec<Condition>,
    /// Stale checks missing from the desired order.
    pub removed_checks: Vec<Condition>,
    /// The timeout edit.
    pub timeout: Option<TimeoutChange>,
}

/// One hook change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookChange {
    /// Added header with full conditions.
    Added,
    /// Removed header alone.
    Removed,
    /// Changed header with detail lines.
    Modified(HookModification),
    /// Silent change with no lines.
    Unchanged,
}

/// One hook lifecycle pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookLifecycle<'a> {
    /// The hook under display.
    pub hook: &'a Hook,
    /// The lifecycle change.
    pub change: HookChange,
}

impl HookModification {
    /// Report whether one modification holds no edits.
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty()
            && self.added_checks.is_empty()
            && self.removed_checks.is_empty()
            && self.timeout.is_none()
    }
}

impl HookChange {
    /// Report whether one change renders no lines.
    pub fn is_silent(&self) -> bool {
        match self {
            Self::Unchanged => true,
            Self::Modified(edits) => edits.is_empty(),
            Self::Added | Self::Removed => false,
        }
    }

    /// Report whether one change renders an added header.
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added)
    }

    /// Report whether one change renders a removed header.
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Report whether one change renders a changed header.
    pub fn is_modified(&self) -> bool {
        match self {
            Self::Modified(edits) => !edits.is_empty(),
            Self::Added | Self::Removed | Self::Unchanged => false,
        }
    }
}

impl HookLifecycle<'_> {
    /// Report whether one lifecycle renders an added header.
    pub fn is_added(&self) -> bool {
        self.change.is_added()
    }

    /// Report whether one lifecycle renders a removed header.
    pub fn is_removed(&self) -> bool {
        self.change.is_removed()
    }

    /// Report whether one lifecycle renders a changed header.
    pub fn is_modified(&self) -> bool {
        self.change.is_modified()
    }

    /// Report whether one lifecycle renders no lines.
    pub fn is_silent(&self) -> bool {
        self.change.is_silent()
    }
}

/// Merge hooks sharing argv and path into one run each.
///
/// - First-seen order wins.
/// - Timeout takes the max.
/// - Single hooks pass through untouched.
pub fn merge_hooks(hooks: Vec<Hook>) -> Vec<Hook> {
    let mut order: Vec<(Vec<Arg>, Vec<Arg>)> = Vec::new();
    let mut merged: Vec<Hook> = Vec::new();
    for hook in hooks {
        let key = (hook.argv.clone(), hook.path.clone());
        match order.iter().position(|held| *held == key) {
            Some(index) => {
                let current = &mut merged[index];
                current.requires = join_requires(current.requires.take(), hook.requires);
                current.when = join_when(current.when.take(), hook.when);
                current.checks.extend(hook.checks);
                current.timeout_secs = current.timeout_secs.max(hook.timeout_secs);
            }
            None => {
                order.push(key);
                merged.push(hook);
            }
        }
    }
    merged
}

/// Diff desired hooks against previous hooks in render order.
///
/// - Current hooks lead in plan order, removals trail in previous order.
pub fn diff_lifecycle<'a>(current: &'a [Hook], previous: &'a [Hook]) -> Vec<HookLifecycle<'a>> {
    let mut out = Vec::new();
    for hook in current {
        match find_recorded(hook, previous) {
            None => out.push(HookLifecycle {
                hook,
                change: HookChange::Added,
            }),
            Some(recorded) => {
                let edits = modify_hook(recorded, hook);
                if edits.is_empty() {
                    out.push(HookLifecycle {
                        hook,
                        change: HookChange::Unchanged,
                    });
                } else {
                    out.push(HookLifecycle {
                        hook,
                        change: HookChange::Modified(edits),
                    });
                }
            }
        }
    }
    for recorded in previous {
        if find_current(recorded, current).is_none() {
            out.push(HookLifecycle {
                hook: recorded,
                change: HookChange::Removed,
            });
        }
    }
    out
}

/// Join two capability conditions with AND semantics.
fn join_requires(first: Option<Condition>, second: Option<Condition>) -> Option<Condition> {
    match (first, second) {
        (None, _) | (_, None) => None,
        (Some(left), Some(right)) => {
            let mut items = flatten_all(left);
            items.extend(flatten_all(right));
            Some(Condition::All(items))
        }
    }
}

/// Flatten one capability condition into its `All` members.
fn flatten_all(cond: Condition) -> Vec<Condition> {
    match cond {
        Condition::All(items) => items,
        other => vec![other],
    }
}

/// Join two need conditions with OR semantics.
fn join_when(first: Option<Condition>, second: Option<Condition>) -> Option<Condition> {
    match (first, second) {
        (None, _) | (_, None) => None,
        (Some(left), Some(right)) => {
            let mut items = flatten_any(left);
            items.extend(flatten_any(right));
            Some(Condition::Any(items))
        }
    }
}

/// Flatten one condition into its `Any` members.
fn flatten_any(cond: Condition) -> Vec<Condition> {
    match cond {
        Condition::Any(items) => items,
        other => vec![other],
    }
}

/// Report whether two hooks share one lifecycle identity on argv and path.
fn same_identity(first: &Hook, second: &Hook) -> bool {
    first.argv == second.argv && first.path == second.path
}

/// Find one recorded hook sharing identity with one desired hook.
fn find_recorded<'a>(hook: &Hook, previous: &'a [Hook]) -> Option<&'a Hook> {
    previous.iter().find(|held| same_identity(held, hook))
}

/// Find one desired hook sharing identity with one recorded hook.
fn find_current<'a>(recorded: &Hook, current: &'a [Hook]) -> Option<&'a Hook> {
    current.iter().find(|hook| same_identity(hook, recorded))
}

/// Build one condition change while simplified conditions differ.
fn condition_change(
    slot: ConditionSlot,
    before: Option<&Condition>,
    after: Option<&Condition>,
) -> Option<ConditionChange> {
    let old = before.map(Condition::simplified);
    let next = after.map(Condition::simplified);
    if old == next {
        return None;
    }
    Some(ConditionChange {
        slot,
        before: before.cloned(),
        after: after.cloned(),
    })
}

/// Build one timeout change while caps differ.
fn timeout_change(before: u64, after: u64) -> Option<TimeoutChange> {
    if before == after {
        return None;
    }
    Some(TimeoutChange { before, after })
}

/// Collect removed checks in previous order.
///
/// - Members match as a bag by simplified form with duplicate counts.
fn removed_checks(before: &[Condition], after: &[Condition]) -> Vec<Condition> {
    let mut rest: Vec<Condition> = after.iter().map(Condition::simplified).collect();
    let mut missing = Vec::new();
    for text in before {
        let key = text.simplified();
        match rest.iter().position(|item| *item == key) {
            Some(index) => {
                rest.remove(index);
            }
            None => missing.push(text.clone()),
        }
    }
    missing
}

/// Collect added checks in desired order.
///
/// - Members match as a bag by simplified form with duplicate counts.
fn added_checks(before: &[Condition], after: &[Condition]) -> Vec<Condition> {
    let mut rest: Vec<Condition> = after.to_vec();
    let mut simple: Vec<Condition> = after.iter().map(Condition::simplified).collect();
    for text in before {
        let key = text.simplified();
        if let Some(index) = simple.iter().position(|item| *item == key) {
            simple.remove(index);
            rest.remove(index);
        }
    }
    rest
}

/// Build one hook modification across conditions, checks, and timeout.
///
/// - Conditions collect in slot order.
fn modify_hook(recorded: &Hook, hook: &Hook) -> HookModification {
    let mut conditions = Vec::new();
    if let Some(change) = condition_change(
        ConditionSlot::Requires,
        recorded.requires.as_ref(),
        hook.requires.as_ref(),
    ) {
        conditions.push(change);
    }
    if let Some(change) = condition_change(
        ConditionSlot::When,
        recorded.when.as_ref(),
        hook.when.as_ref(),
    ) {
        conditions.push(change);
    }
    HookModification {
        conditions,
        removed_checks: removed_checks(&recorded.checks, &hook.checks),
        added_checks: added_checks(&recorded.checks, &hook.checks),
        timeout: timeout_change(recorded.timeout_secs, hook.timeout_secs),
    }
}

/// Return the run cap for an absent timeout.
fn default_timeout_secs() -> u64 {
    600
}

/// Reject hooks without commands.
///
/// # Errors
///
/// - [`mlua::Error`] for empty argv.
fn reject_empty_argv(hook: &Hook) -> mlua::Result<()> {
    if hook.argv.is_empty() {
        return Err(mlua::Error::external(
            "invalid hook argv 'table': want a non-empty string-or-route array",
        ));
    }
    Ok(())
}

/// Decode one duration string into seconds.
///
/// # Errors
///
/// - [`Mismatch`] for values outside the duration shape.
fn decode_timeout(lua: &Lua, value: &Value) -> std::result::Result<u64, Mismatch> {
    let text = String::decode(value, lua)?;
    parse_timeout(&text).ok_or_else(|| Mismatch {
        path: "timeout".to_owned(),
        want: "a duration like '10m'".to_owned(),
        found: ["'", text.as_str(), "'"].concat(),
    })
}

/// Render timeout seconds as bare digits.
fn render_timeout(timeout: u64) -> String {
    timeout.to_string()
}

/// Parse one duration string into seconds.
///
/// Bare digits read seconds. Suffixed parts read hours, minutes,
/// and seconds in descending order with each unit at most once.
fn parse_timeout(text: &str) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    let mut total: u64 = 0;
    let mut rank: u8 = 4;
    let mut seen: u8 = 0;
    let mut rest = text;
    let mut consumed = false;
    while !rest.is_empty() {
        let end = rest
            .find(|part: char| !part.is_ascii_digit())
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        let amount: u64 = rest[..end].parse().ok()?;
        rest = &rest[end..];
        match rest.chars().next() {
            None => {
                if consumed {
                    return None;
                }
                total = amount;
                consumed = true;
            }
            Some(unit) => {
                let (unit_rank, unit_bit, factor) = match unit {
                    'h' => (3, 0b100, 3_600),
                    'm' => (2, 0b010, 60),
                    's' => (1, 0b001, 1),
                    _ => return None,
                };
                if unit_rank >= rank || seen & unit_bit != 0 {
                    return None;
                }
                rank = unit_rank;
                seen |= unit_bit;
                total = total.checked_add(amount.checked_mul(factor)?)?;
                rest = &rest[unit.len_utf8()..];
                consumed = true;
            }
        }
    }
    if consumed { Some(total) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hook(
        argv: &[&str],
        path: &[&str],
        when: Option<Condition>,
        checks: Vec<Condition>,
        timeout_secs: u64,
    ) -> Hook {
        Hook {
            argv: argv
                .iter()
                .map(|item| Arg::Text(item.to_string()))
                .collect(),
            path: path
                .iter()
                .map(|item| Arg::Text(item.to_string()))
                .collect(),
            requires: None,
            when,
            checks,
            timeout_secs,
        }
    }

    fn in_path(name: &str) -> Condition {
        Condition::InPath {
            name: name.to_string(),
        }
    }

    #[test]
    fn identical_hooks_collapse_keeping_first_order() {
        let first = hook(&["mise", "install"], &[], None, vec![], 60);
        let second = hook(&["mise", "install"], &[], None, vec![], 60);
        let third = hook(&["fc-cache"], &[], None, vec![], 60);
        let merged = merge_hooks(vec![first.clone(), second, third.clone()]);
        assert_eq!(merged, vec![first, third]);
    }

    #[test]
    fn differing_path_runs_separately() {
        let merged = merge_hooks(vec![
            hook(&["mise"], &["/a"], None, vec![], 60),
            hook(&["mise"], &["/b"], None, vec![], 60),
        ]);
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn conditions_join_with_or_flattening_any() {
        let merged = merge_hooks(vec![
            hook(
                &["mise"],
                &[],
                Some(Condition::Any(vec![in_path("a"), in_path("b")])),
                vec![],
                60,
            ),
            hook(&["mise"], &[], Some(in_path("c")), vec![], 60),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].when,
            Some(Condition::Any(vec![
                in_path("a"),
                in_path("b"),
                in_path("c")
            ]))
        );
    }

    #[test]
    fn absent_side_keeps_merged_hook_absent() {
        let merged = merge_hooks(vec![
            hook(&["mise"], &[], Some(in_path("a")), vec![], 60),
            hook(&["mise"], &[], None, vec![], 60),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].when, None);
    }

    #[test]
    fn checks_concatenate_and_timeout_takes_max() {
        let merged = merge_hooks(vec![
            hook(&["mise"], &[], None, vec![in_path("a")], 60),
            hook(&["mise"], &[], None, vec![in_path("b")], 600),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].checks, vec![in_path("a"), in_path("b")]);
        assert_eq!(merged[0].timeout_secs, 600);
    }

    #[test]
    fn single_condition_hook_passes_through_untouched() {
        let single = hook(&["mise"], &[], Some(in_path("a")), vec![in_path("b")], 60);
        let merged = merge_hooks(vec![single.clone()]);
        assert_eq!(merged, vec![single]);
    }

    #[test]
    fn requires_merge_joins_with_and_flattening_all() {
        let mut first = hook(&["mise"], &[], None, vec![], 60);
        first.requires = Some(Condition::All(vec![in_path("a"), in_path("b")]));
        let mut second = hook(&["mise"], &[], None, vec![], 60);
        second.requires = Some(in_path("c"));
        let merged = merge_hooks(vec![first, second]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].requires,
            Some(Condition::All(vec![
                in_path("a"),
                in_path("b"),
                in_path("c")
            ]))
        );
    }

    #[test]
    fn when_merge_joins_distinct_leaves_with_or() {
        let merged = merge_hooks(vec![
            hook(&["mise"], &[], Some(in_path("a")), vec![], 60),
            hook(&["mise"], &[], Some(in_path("b")), vec![], 60),
        ]);
        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].when,
            Some(Condition::Any(vec![in_path("a"), in_path("b")]))
        );
    }

    #[test]
    fn requires_merge_collapses_identical_leaves() {
        let mut first = hook(&["mise"], &[], None, vec![], 60);
        first.requires = Some(in_path("mise"));
        let mut second = hook(&["mise"], &[], None, vec![], 60);
        second.requires = Some(in_path("mise"));
        let merged = merge_hooks(vec![first, second]);
        assert_eq!(merged.len(), 1);
        let condition = match merged[0].requires.clone() {
            Some(condition) => condition,
            None => panic!("requires survives the merge"),
        };
        assert_eq!(condition.simplified(), in_path("mise"));
    }

    #[test]
    fn lifecycle_marks_added_removed_unchanged() {
        let kept = hook(&["tool"], &[], None, vec![], 60);
        let current = vec![kept.clone(), hook(&["fresh"], &[], None, vec![], 60)];
        let previous = vec![kept.clone(), hook(&["stale"], &[], None, vec![], 60)];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 3);
        assert!(lifecycle[0].is_silent());
        assert!(lifecycle[1].is_added());
        assert!(lifecycle[2].is_removed());
    }

    #[test]
    fn lifecycle_reads_simplified_conditions_silent() {
        let mut before = hook(&["mise"], &[], None, vec![], 60);
        before.requires = Some(Condition::All(vec![in_path("mise"), in_path("mise")]));
        let mut after = hook(&["mise"], &[], None, vec![], 60);
        after.requires = Some(in_path("mise"));
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        assert!(lifecycle[0].is_silent());
    }

    #[test]
    fn lifecycle_splits_check_bags_by_count() {
        let before = hook(&["mise"], &[], None, vec![in_path("a")], 60);
        let after = hook(&["mise"], &[], None, vec![in_path("b")], 60);
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        let edits = match &lifecycle[0].change {
            HookChange::Modified(edits) => edits,
            _ => panic!("checks diff is modified"),
        };
        assert_eq!(edits.removed_checks, vec![in_path("a")]);
        assert_eq!(edits.added_checks, vec![in_path("b")]);
    }

    #[test]
    fn lifecycle_holds_timeout_edit() {
        let before = hook(&["mise"], &[], None, vec![], 60);
        let after = hook(&["mise"], &[], None, vec![], 600);
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        assert!(lifecycle[0].is_modified());
        let edits = match &lifecycle[0].change {
            HookChange::Modified(edits) => edits,
            _ => panic!("timeout diff is modified"),
        };
        assert_eq!(
            edits.timeout,
            Some(TimeoutChange {
                before: 60,
                after: 600
            })
        );
    }

    #[test]
    fn timeout_round_trips_through_strings() {
        use confit_binding::value::Encode;

        let lua = Lua::new();
        let source = hook(&["tool"], &[], None, vec![], 90);
        let encoded = source.encode(&lua).unwrap();
        let Value::Table(table) = encoded else {
            panic!("hook encodes a table");
        };
        let rendered: String = table.get("timeout").unwrap();
        assert_eq!(rendered, "90");
        let back = Hook::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(back, source);
    }
}
