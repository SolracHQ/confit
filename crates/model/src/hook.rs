//! Hook
//! Post-config steps riding plans beside documents.

use serde::{Deserialize, Serialize};

use crate::arg::Arg;
use crate::condition::Condition;

/// One post-config step with gates and checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    /// Holds the command and arguments in order.
    pub argv: Vec<Arg>,
    /// Holds the PATH extension dirs for the subprocess alone.
    pub path: Vec<Arg>,
    /// The capability gate.
    #[serde(default)]
    pub requires: Option<Condition>,
    /// The need gate.
    pub when: Option<Condition>,
    /// Holds the proof conditions, AND by list.
    pub checks: Vec<Condition>,
    /// The run cap in seconds.
    pub timeout_secs: u64,
}

/// One gate slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateSlot {
    /// The capability gate.
    Requires,
    /// The need gate.
    When,
}

/// One gate change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateChange {
    /// The slot under diffing.
    pub slot: GateSlot,
    /// The recorded gate.
    pub before: Option<Condition>,
    /// The desired gate.
    pub after: Option<Condition>,
}

/// One timeout change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutChange {
    /// The recorded cap in seconds.
    pub before: u64,
    /// The desired cap in seconds.
    pub after: u64,
}

/// One hook modification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookModification {
    /// Holds the changed gates in slot order.
    pub gates: Vec<GateChange>,
    /// Holds the desired checks missing from recorded order.
    pub added_checks: Vec<Condition>,
    /// Holds the recorded checks missing from desired order.
    pub removed_checks: Vec<Condition>,
    /// The timeout edit.
    pub timeout: Option<TimeoutChange>,
}

/// One hook change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookChange {
    /// Renders one added header with full gates.
    Added,
    /// Renders one removed header alone.
    Removed,
    /// Renders one changed header with detail lines.
    Modified(HookModification),
    /// Renders nothing.
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
    /// Reports whether one modification holds no edits.
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
            && self.added_checks.is_empty()
            && self.removed_checks.is_empty()
            && self.timeout.is_none()
    }
}

impl HookChange {
    /// Reports whether one change renders no lines.
    pub fn is_silent(&self) -> bool {
        match self {
            Self::Unchanged => true,
            Self::Modified(edits) => edits.is_empty(),
            Self::Added | Self::Removed => false,
        }
    }

    /// Reports whether one change renders an added header.
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added)
    }

    /// Reports whether one change renders a removed header.
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Reports whether one change renders a changed header.
    pub fn is_modified(&self) -> bool {
        match self {
            Self::Modified(edits) => !edits.is_empty(),
            Self::Added | Self::Removed | Self::Unchanged => false,
        }
    }
}

impl HookLifecycle<'_> {
    /// Reports whether one lifecycle renders an added header.
    pub fn is_added(&self) -> bool {
        self.change.is_added()
    }

    /// Reports whether one lifecycle renders a removed header.
    pub fn is_removed(&self) -> bool {
        self.change.is_removed()
    }

    /// Reports whether one lifecycle renders a changed header.
    pub fn is_modified(&self) -> bool {
        self.change.is_modified()
    }

    /// Reports whether one lifecycle renders no lines.
    pub fn is_silent(&self) -> bool {
        self.change.is_silent()
    }
}

/// Merges hooks sharing argv and path into one run each.
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
                current.when = join_gate(current.when.take(), hook.when);
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

/// Diffs desired hooks against previous hooks in render order.
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
        if find_live(recorded, current).is_none() {
            out.push(HookLifecycle {
                hook: recorded,
                change: HookChange::Removed,
            });
        }
    }
    out
}

/// Joins two capability gates with AND semantics.
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

/// Flattens one capability gate into its `All` members.
fn flatten_all(cond: Condition) -> Vec<Condition> {
    match cond {
        Condition::All(items) => items,
        other => vec![other],
    }
}

/// Joins two gates with OR semantics.
fn join_gate(first: Option<Condition>, second: Option<Condition>) -> Option<Condition> {
    match (first, second) {
        (None, _) | (_, None) => None,
        (Some(left), Some(right)) => {
            let mut items = flatten_any(left);
            items.extend(flatten_any(right));
            Some(Condition::Any(items))
        }
    }
}

/// Flattens one gate into its `Any` members.
fn flatten_any(cond: Condition) -> Vec<Condition> {
    match cond {
        Condition::Any(items) => items,
        other => vec![other],
    }
}

/// Reports whether two hooks share one lifecycle identity on argv and path.
fn same_identity(first: &Hook, second: &Hook) -> bool {
    first.argv == second.argv && first.path == second.path
}

/// Finds one recorded hook sharing identity with one desired hook.
fn find_recorded<'a>(hook: &Hook, previous: &'a [Hook]) -> Option<&'a Hook> {
    previous.iter().find(|held| same_identity(held, hook))
}

/// Finds one desired hook sharing identity with one recorded hook.
fn find_live<'a>(recorded: &Hook, current: &'a [Hook]) -> Option<&'a Hook> {
    current.iter().find(|hook| same_identity(hook, recorded))
}

/// Builds one gate change while simplified gates differ.
fn gate_change(
    slot: GateSlot,
    before: Option<&Condition>,
    after: Option<&Condition>,
) -> Option<GateChange> {
    let old = before.map(Condition::simplified);
    let next = after.map(Condition::simplified);
    if old == next {
        return None;
    }
    Some(GateChange {
        slot,
        before: before.cloned(),
        after: after.cloned(),
    })
}

/// Builds one timeout change while caps differ.
fn timeout_change(before: u64, after: u64) -> Option<TimeoutChange> {
    if before == after {
        return None;
    }
    Some(TimeoutChange { before, after })
}

/// Collects removed checks in previous order.
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

/// Collects added checks in desired order.
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

/// Builds one hook modification across gates, checks, and timeout.
///
/// - Gates collect in slot order.
fn modify_hook(recorded: &Hook, hook: &Hook) -> HookModification {
    let mut gates = Vec::new();
    if let Some(change) = gate_change(
        GateSlot::Requires,
        recorded.requires.as_ref(),
        hook.requires.as_ref(),
    ) {
        gates.push(change);
    }
    if let Some(change) = gate_change(GateSlot::When, recorded.when.as_ref(), hook.when.as_ref()) {
        gates.push(change);
    }
    HookModification {
        gates,
        removed_checks: removed_checks(&recorded.checks, &hook.checks),
        added_checks: added_checks(&recorded.checks, &hook.checks),
        timeout: timeout_change(recorded.timeout_secs, hook.timeout_secs),
    }
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
    fn gates_join_with_or_flattening_any() {
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
    fn ungated_side_keeps_merged_hook_ungated() {
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
    fn single_gated_hook_passes_through_untouched() {
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
        let gate = match merged[0].requires.clone() {
            Some(gate) => gate,
            None => panic!("requires survives the merge"),
        };
        assert_eq!(gate.simplified(), in_path("mise"));
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
    fn lifecycle_reads_simplified_gates_silent() {
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
            _ => panic!("checks diff reads modified"),
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
            _ => panic!("timeout diff reads modified"),
        };
        assert_eq!(
            edits.timeout,
            Some(TimeoutChange {
                before: 60,
                after: 600
            })
        );
    }
}
