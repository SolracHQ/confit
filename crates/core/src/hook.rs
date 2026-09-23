//! Hook
//!
//! Post-config steps riding plans beside documents.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::condition::Condition;
use crate::probe::PathProbe;
use crate::runtime::Runtime;

/// One post-config step with gates and checks.
///
/// Argv executes directly with no shell in between. Path
/// extends PATH for the hook subprocess alone. Requires gates
/// the run on capability, when gates the run on need, each
/// holding runtime conditions. Checks prove the run with
/// the same shapes: passing checks skip the hook, failing
/// checks run it. Timeout caps the run in seconds.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    /// Holds the command and arguments in order.
    pub argv: Vec<String>,
    /// Holds the PATH extension dirs for the subprocess alone.
    pub path: Vec<String>,
    /// Holds the capability gate. None runs where capable.
    #[serde(default)]
    pub requires: Option<Condition>,
    /// Holds the need gate. None runs unconditionally.
    pub when: Option<Condition>,
    /// Holds the proof conditions, AND by list.
    pub checks: Vec<Condition>,
    /// Holds the run cap in seconds.
    pub timeout_secs: u64,
}

/// One gate slot holding the changed gate.
///
/// Requires gates capability, when gates need. The slot
/// selects the rendered gate line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateSlot {
    /// Holds the capability gate.
    Requires,
    /// Holds the need gate.
    When,
}

/// One gate change holding before and after gates.
///
/// Either side reads `None` while absent. Equal simplified
/// gates never build this shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateChange {
    /// Holds the slot under diffing.
    pub slot: GateSlot,
    /// Holds the recorded gate. None reads absent.
    pub before: Option<Condition>,
    /// Holds the desired gate. None reads absent.
    pub after: Option<Condition>,
}

/// One timeout change holding before and after caps.
///
/// Equal caps never build this shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutChange {
    /// Holds the recorded cap in seconds.
    pub before: u64,
    /// Holds the desired cap in seconds.
    pub after: u64,
}

/// One hook modification holding gate, check, and timeout edits.
///
/// Empty gates with empty check edits and no timeout reads
/// silent and never renders.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookModification {
    /// Holds the changed gates in slot order.
    pub gates: Vec<GateChange>,
    /// Holds the desired checks missing from recorded order.
    pub added_checks: Vec<Condition>,
    /// Holds the recorded checks missing from desired order.
    pub removed_checks: Vec<Condition>,
    /// Holds the timeout edit. None reads unchanged.
    pub timeout: Option<TimeoutChange>,
}

/// One hook change selecting the lifecycle lines.
///
/// Added and removed hooks render one header line. Modified
/// hooks render one header with detail lines. Unchanged hooks
/// render nothing.
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

/// One hook lifecycle pairing its hook with its change.
///
/// The hook borrows core data, the change carries the diff
/// facts. Rendering reads the pair alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookLifecycle<'a> {
    /// Holds the hook under display.
    pub hook: &'a Hook,
    /// Holds the lifecycle change behind its lines.
    pub change: HookChange,
}

impl HookModification {
    /// Reports whether one modification holds no edits.
    ///
    /// # Returns
    ///
    /// True while gates, check edits, and timeout all read empty.
    pub fn is_empty(&self) -> bool {
        self.gates.is_empty()
            && self.added_checks.is_empty()
            && self.removed_checks.is_empty()
            && self.timeout.is_none()
    }
}

impl HookChange {
    /// Reports whether one change renders no lines.
    ///
    /// # Returns
    ///
    /// True for unchanged hooks and empty modifications.
    pub fn is_silent(&self) -> bool {
        match self {
            Self::Unchanged => true,
            Self::Modified(edits) => edits.is_empty(),
            Self::Added | Self::Removed => false,
        }
    }

    /// Reports whether one change renders an added header.
    ///
    /// # Returns
    ///
    /// True while the hook reads added.
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added)
    }

    /// Reports whether one change renders a removed header.
    ///
    /// # Returns
    ///
    /// True while the hook reads removed.
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Reports whether one change renders a changed header.
    ///
    /// # Returns
    ///
    /// True while the hook reads modified with edits.
    pub fn is_modified(&self) -> bool {
        match self {
            Self::Modified(edits) => !edits.is_empty(),
            Self::Added | Self::Removed | Self::Unchanged => false,
        }
    }
}

impl HookLifecycle<'_> {
    /// Reports whether one lifecycle renders an added header.
    ///
    /// # Returns
    ///
    /// True while the change reads added.
    pub fn is_added(&self) -> bool {
        self.change.is_added()
    }

    /// Reports whether one lifecycle renders a removed header.
    ///
    /// # Returns
    ///
    /// True while the change reads removed.
    pub fn is_removed(&self) -> bool {
        self.change.is_removed()
    }

    /// Reports whether one lifecycle renders a changed header.
    ///
    /// # Returns
    ///
    /// True while the change reads modified with edits.
    pub fn is_modified(&self) -> bool {
        self.change.is_modified()
    }

    /// Reports whether one lifecycle renders no lines.
    ///
    /// # Returns
    ///
    /// True while the change reads silent.
    pub fn is_silent(&self) -> bool {
        self.change.is_silent()
    }
}

/// Merges hooks sharing argv and path into one run each.
///
/// First-seen order wins. Gates join across both sides, checks
/// concatenate, timeout takes the max. Single hooks pass
/// through untouched.
///
/// # Arguments
///
/// * `hooks` - the declared hooks in declaration order.
///
/// # Returns
///
/// The merged hooks in first-seen order.
///
pub fn merge_hooks(hooks: Vec<Hook>) -> Vec<Hook> {
    let mut order: Vec<(Vec<String>, Vec<String>)> = Vec::new();
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
/// Current hooks lead in plan order, removals trail in
/// previous order. Identity joins on argv and path. Gates
/// compare by simplified form, checks compare as a bag by
/// simplified form with duplicate counts, timeout compares by
/// value. Equal hooks read silent.
///
/// # Arguments
///
/// * `current` - the desired hooks in plan order.
/// * `previous` - the previous hooks backing lifecycle marks.
///
/// # Returns
///
/// The lifecycle entries in plan order with removals trailing.
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

/// Resolves one hook binary across hook path dirs and runtime dirs.
///
/// Hook path entries search first, runtime dirs follow. First
/// existing executable wins with the `in_path` rule.
///
/// # Arguments
///
/// * `hook` - the hook holding argv and path dirs.
/// * `rt` - the runtime facts under reading.
/// * `probe` - the probe under stating.
///
/// # Returns
///
/// The joined candidate path for the first hit, else `None`.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_core::hook::{Hook, resolve_hook};
/// use confit_core::probe::MemoryProbe;
/// use confit_core::runtime::Runtime;
///
/// let mut probe = MemoryProbe::new();
/// probe.exec(std::path::Path::new("/opt/tool"));
/// let hook = Hook {
///     argv: vec!["tool".into()],
///     path: Vec::new(),
///     requires: None,
///     when: None,
///     checks: Vec::new(),
///     timeout_secs: 600,
/// };
/// let rt = Runtime {
///     vars: Default::default(),
///     path_dirs: vec![std::path::PathBuf::from("/opt")],
/// };
/// assert_eq!(
///     resolve_hook(&hook, &rt, &probe),
///     Some(std::path::PathBuf::from("/opt/tool"))
/// );
/// ```
pub fn resolve_hook(hook: &Hook, rt: &Runtime, probe: &dyn PathProbe) -> Option<PathBuf> {
    let head = hook.argv.first()?;
    let mut dirs: Vec<PathBuf> = hook.path.iter().map(PathBuf::from).collect();
    dirs.extend(rt.path_dirs.iter().cloned());
    probe.find_executable(head, &dirs)
}

/// Reports whether two hooks share one lifecycle identity.
///
/// Identity joins on argv and path, so gates, checks, and
/// timeout never split one run.
fn same_identity(first: &Hook, second: &Hook) -> bool {
    first.argv == second.argv && first.path == second.path
}

/// Finds one recorded hook sharing identity with one desired hook.
///
/// # Returns
///
/// The recorded hook holding the same argv and path, else `None`.
fn find_recorded<'a>(hook: &Hook, previous: &'a [Hook]) -> Option<&'a Hook> {
    previous.iter().find(|held| same_identity(held, hook))
}

/// Finds one desired hook sharing identity with one recorded hook.
///
/// # Returns
///
/// The desired hook holding the same argv and path, else `None`.
fn find_live<'a>(recorded: &Hook, current: &'a [Hook]) -> Option<&'a Hook> {
    current.iter().find(|hook| same_identity(hook, recorded))
}

/// Reads one gate change while simplified gates differ.
///
/// # Returns
///
/// The change holding original gates, else `None` while equal.
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

/// Reads one timeout change while caps differ.
///
/// # Returns
///
/// The change holding both caps, else `None` while equal.
fn timeout_change(before: u64, after: u64) -> Option<TimeoutChange> {
    if before == after {
        return None;
    }
    Some(TimeoutChange { before, after })
}

/// Collects removed checks in previous order.
///
/// Members match as a bag by simplified form, so duplicate
/// gates diff by count. Matched desired members consume one
/// previous member each.
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
/// Members match as a bag by simplified form, so duplicate
/// gates diff by count. Matched recorded members consume one
/// desired member each.
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

/// Reads one hook modification across gates, checks, and timeout.
///
/// Gates collect in slot order, checks split into removed
/// then added bags, timeout holds the cap edit. Empty edits
/// read silent.
///
/// # Returns
///
/// The modification holding every edit.
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
            argv: argv.iter().map(|item| item.to_string()).collect(),
            path: path.iter().map(|item| item.to_string()).collect(),
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
