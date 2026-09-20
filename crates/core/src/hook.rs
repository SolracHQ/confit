//! Hook
//!
//! Post-config steps riding plans beside documents.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::condition::Condition;
use crate::fs::Filesystem;
use crate::ids::DocPath;
use crate::runtime::{Runtime, evaluate, find_binary};

/// One post-config step with gates plus checks.
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
    /// Holds the command plus arguments in order.
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

/// Merges hooks sharing argv plus path into one run each.
///
/// First-seen order wins. Requires gates join with AND:
/// `All` of the two sides, flattening nested `All`. When
/// gates join with OR: `Any` of the two sides, flattening
/// nested `Any`. An ungated side keeps the merged hook
/// ungated on that slot. Checks concatenate. Timeout
/// takes the max. Single hooks pass through untouched, so
/// the common case keeps its bare shape.
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

/// Resolves one hook binary across hook path dirs plus runtime dirs.
///
/// Hook path entries search first, runtime dirs follow. First
/// existing executable wins with the `in_path` rule.
///
/// # Arguments
///
/// * `hook` - the hook holding argv plus path dirs.
/// * `rt` - the runtime facts under reading.
/// * `fs` - the backend under stating.
///
/// # Returns
///
/// The joined candidate path for the first hit, else `None`.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::hook::{Hook, resolve_hook};
/// use confit_core::runtime::Runtime;
///
/// let fs = MemoryFs::new();
/// let binary = std::path::Path::new("/opt/tool");
/// assert!(matches!(fs.write(binary, b"run"), Ok(())));
/// assert!(matches!(fs.set_mode(binary, 0o755), Ok(())));
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
///     resolve_hook(&hook, &rt, &fs),
///     Some(std::path::PathBuf::from("/opt/tool"))
/// );
/// ```
pub fn resolve_hook(hook: &Hook, rt: &Runtime, fs: &dyn Filesystem) -> Option<PathBuf> {
    let head = hook.argv.first()?;
    let mut dirs: Vec<PathBuf> = hook.path.iter().map(PathBuf::from).collect();
    dirs.extend(rt.path_dirs.iter().cloned());
    find_binary(head, &dirs, fs)
}

/// Reports whether one hook reads satisfied with passing checks.
fn checks_pass(
    hook: &Hook,
    rt: &Runtime,
    fs: &dyn Filesystem,
    changed: &BTreeSet<DocPath>,
) -> bool {
    !hook.checks.is_empty()
        && hook
            .checks
            .iter()
            .all(|check| evaluate(check, rt, fs, changed))
}

/// Reads one hook argv as display text.
fn argv_text(hook: &Hook) -> String {
    hook.argv.join(" ")
}

/// Renders one preview line for a runnable hook.
fn run_line(hook: &Hook, binary: &std::path::Path) -> String {
    let rest = hook
        .argv
        .iter()
        .skip(1)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    if rest.is_empty() {
        format!("! run: {}", binary.display())
    } else {
        format!("! run: {} {rest}", binary.display())
    }
}

/// Renders one preview line for a single hook in plan order.
///
/// Closed requires warns with the gate named. Closed when
/// skips with the un-need named. Satisfied checks skip with
/// the work standing done. Everything else resolves the
/// binary and runs. First closed mouth speaks.
///
/// # Arguments
///
/// * `hook` - the hook under preview.
/// * `rt` - the runtime facts under reading.
/// * `fs` - the backend under stating.
/// * `changed` - the changed document ids under reading.
///
/// # Returns
///
/// The preview line for the hook.
///
/// # Errors
///
/// Unresolvable binaries fail as plan errors naming the hook.
pub fn preview_hook(
    hook: &Hook,
    rt: &Runtime,
    fs: &dyn Filesystem,
    changed: &BTreeSet<DocPath>,
) -> crate::error::Result<String> {
    if let Some(gate) = hook.requires.as_ref()
        && !evaluate(gate, rt, fs, changed)
    {
        return Ok(format!(
            "warn: {} cannot run ({})",
            argv_text(hook),
            describe_condition(gate)
        ));
    }
    if let Some(gate) = hook.when.as_ref()
        && !evaluate(gate, rt, fs, changed)
    {
        return Ok(format!(
            "skipped: {} (no need: {})",
            argv_text(hook),
            describe_condition(gate)
        ));
    }
    if checks_pass(hook, rt, fs, changed) {
        return Ok(format!("skipped: {} (checks pass)", argv_text(hook)));
    }
    let Some(binary) = resolve_hook(hook, rt, fs) else {
        let head = hook.argv.first().cloned().unwrap_or_default();
        return Err(crate::error::Error::Plan(format!(
            "hook '{}' cannot resolve '{head}'",
            argv_text(hook)
        )));
    };
    Ok(run_line(hook, &binary))
}

/// Renders one condition in compact form for warn lines.
///
/// Simplifies before rendering, so merged duplicate gates collapse
/// to one branch. Compounds read infix with full parens, leaves read
/// bare.
///
/// # Arguments
///
/// * `cond` - the condition under rendering.
///
/// # Returns
///
/// The compact gate text like `(in_path(mise) and changed(path))`.
///
/// # Examples
///
/// ```rust
/// use confit_core::condition::Condition;
/// use confit_core::hook::describe_condition;
///
/// let cond = Condition::InPath { name: "mise".into() };
/// assert!(matches!(describe_condition(&cond).as_str(), "in_path(mise)"));
/// ```
pub fn describe_condition(cond: &Condition) -> String {
    fn render(cond: &Condition) -> String {
        match cond {
            Condition::EnvEq { key, value } => format!("env_eq({key}={value})"),
            Condition::EnvSet { key } => format!("env_set({key})"),
            Condition::InPath { name } => format!("in_path({name})"),
            Condition::Exists { path } => format!("exists({path})"),
            Condition::Changed { path } => format!("changed({path})"),
            Condition::All(items) if items.is_empty() => "true".to_string(),
            Condition::Any(items) if items.is_empty() => "false".to_string(),
            Condition::All(items) => format!(
                "({})",
                items.iter().map(render).collect::<Vec<_>>().join(" and ")
            ),
            Condition::Any(items) => format!(
                "({})",
                items.iter().map(render).collect::<Vec<_>>().join(" or ")
            ),
            Condition::Not(inner) => format!("(not {})", render(inner)),
        }
    }
    render(&cond.simplified())
}

/// Renders plan hook lines for desired hooks against previous hooks.
///
/// # Arguments
///
/// * `current` - the desired hooks in plan order.
/// * `previous` - the previous hooks backing lifecycle marks.
///
/// # Returns
///
/// The lifecycle lines in plan order with removals trailing.
///
/// # Examples
///
/// ```rust
/// use confit_core::hook::lifecycle_lines;
///
/// assert!(matches!(lifecycle_lines(&[], &[]).is_empty(), true));
/// ```
pub fn lifecycle_lines(current: &[Hook], previous: &[Hook]) -> Vec<String> {
    let mut lines = Vec::new();
    for hook in current {
        let recorded = previous
            .iter()
            .find(|held| held.argv == hook.argv && held.path == hook.path);
        match recorded {
            None => {
                lines.push(format!("+ {}", argv_text(hook)));
                full_gates(hook, &mut lines);
            }
            Some(recorded) => {
                let mut diff = Vec::new();
                diff_gate(
                    "requires",
                    recorded.requires.as_ref(),
                    hook.requires.as_ref(),
                    &mut diff,
                );
                diff_gate(
                    "when",
                    recorded.when.as_ref(),
                    hook.when.as_ref(),
                    &mut diff,
                );
                diff_checks(&recorded.checks, &hook.checks, &mut diff);
                if recorded.timeout_secs != hook.timeout_secs {
                    diff.push(format!(
                        "  ~ timeout ({} -> {})",
                        timeout_text(recorded.timeout_secs),
                        timeout_text(hook.timeout_secs)
                    ));
                }
                if !diff.is_empty() {
                    lines.push(format!("~ {}", argv_text(hook)));
                    lines.extend(diff);
                }
            }
        }
    }
    for recorded in previous {
        let live = current
            .iter()
            .any(|hook| hook.argv == recorded.argv && hook.path == recorded.path);
        if !live {
            lines.push(format!("- {}", argv_text(recorded)));
        }
    }
    lines
}

/// Renders full gate lines for one added hook.
///
/// Requires plus when print once while present, checks print
/// once per member, all marked new in slot order.
fn full_gates(hook: &Hook, out: &mut Vec<String>) {
    if let Some(gate) = hook.requires.as_ref() {
        out.push(format!("  + requires ({})", describe_condition(gate)));
    }
    if let Some(gate) = hook.when.as_ref() {
        out.push(format!("  + when ({})", describe_condition(gate)));
    }
    for check in &hook.checks {
        out.push(format!("  + checks ({})", describe_condition(check)));
    }
}

/// Renders one gate diff line for a single slot while text differs.
///
/// Absent-to-present reads added, present-to-absent reads
/// removed, changed reads old-to-new. Equal describe text
/// reads silent.
fn diff_gate(slot: &str, old: Option<&Condition>, new: Option<&Condition>, out: &mut Vec<String>) {
    let prior = old.map(describe_condition);
    let next = new.map(describe_condition);
    match (prior, next) {
        (Some(before), Some(after)) if before == after => {}
        (Some(before), Some(after)) => {
            out.push(format!("  ~ {slot} ({before}) -> ({after})"));
        }
        (None, Some(after)) => {
            out.push(format!("  + {slot} ({after})"));
        }
        (Some(before), None) => {
            out.push(format!("  - {slot} ({before})"));
        }
        (None, None) => {}
    }
}

/// Renders added plus removed check lines by describe text.
///
/// Members compare as a bag, so duplicate gates diff by
/// count. Removals print in previous order, additions print
/// in desired order.
fn diff_checks(old: &[Condition], next: &[Condition], out: &mut Vec<String>) {
    let mut rest: Vec<String> = next.iter().map(describe_condition).collect();
    let mut missing = Vec::new();
    for text in old.iter().map(describe_condition) {
        match rest.iter().position(|item| *item == text) {
            Some(index) => {
                rest.remove(index);
            }
            None => missing.push(text),
        }
    }
    for text in missing {
        out.push(format!("  - checks ({text})"));
    }
    for text in rest {
        out.push(format!("  + checks ({text})"));
    }
}

/// Renders one timeout with the run cap suffix.
///
/// Matches the timed-out hook error shape, so `600` reads
/// as `600s` here too.
fn timeout_text(secs: u64) -> String {
    format!("{secs}s")
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
        assert_eq!(describe_condition(&gate).as_str(), "in_path(mise)");
    }

    fn changed(path: &str) -> Condition {
        Condition::Changed {
            path: path.to_string(),
        }
    }

    #[test]
    fn describe_collapses_merged_duplicate_gates() {
        let gate = || Condition::All(vec![in_path("mise"), changed("~/.config/mise/config.toml")]);
        let merged = Condition::Any(vec![gate(), gate(), gate(), gate(), gate()]);
        assert_eq!(
            describe_condition(&merged).as_str(),
            "(in_path(mise) and changed(~/.config/mise/config.toml))"
        );
    }

    #[test]
    fn describe_absorbs_redundant_branches() {
        let gate = Condition::All(vec![
            in_path("mise"),
            Condition::Any(vec![in_path("mise"), changed("dest")]),
        ]);
        assert_eq!(describe_condition(&gate).as_str(), "in_path(mise)");
    }

    #[test]
    fn describe_nests_compounds_with_full_parens() {
        let gate = Condition::Any(vec![
            Condition::All(vec![in_path("a"), in_path("b")]),
            Condition::Not(Box::new(in_path("c"))),
        ]);
        assert_eq!(
            describe_condition(&gate).as_str(),
            "((in_path(a) and in_path(b)) or (not in_path(c)))"
        );
    }

    #[test]
    fn describe_folds_empty_lists_to_constants() {
        assert_eq!(
            describe_condition(&Condition::All(Vec::new())).as_str(),
            "true"
        );
        assert_eq!(
            describe_condition(&Condition::Any(Vec::new())).as_str(),
            "false"
        );
    }

    fn preview_state() -> (Runtime, crate::fs::MemoryFs) {
        let fs = crate::fs::MemoryFs::new();
        let _ = fs.write(std::path::Path::new("/opt/tool"), b"run");
        let _ = fs.set_mode(std::path::Path::new("/opt/tool"), 0o755);
        let _ = fs.write(std::path::Path::new("/opt/probe"), b"done");
        let rt = Runtime {
            vars: Default::default(),
            path_dirs: vec![std::path::PathBuf::from("/opt")],
        };
        (rt, fs)
    }

    fn changed_set(dests: &[&str]) -> BTreeSet<DocPath> {
        dests.iter().copied().map(DocPath::new).collect()
    }

    fn preview_line(
        hook: &Hook,
        rt: &Runtime,
        fs: &dyn Filesystem,
        changed: &BTreeSet<DocPath>,
    ) -> String {
        match preview_hook(hook, rt, fs, changed) {
            Ok(line) => line,
            Err(error) => panic!("preview renders: {error}"),
        }
    }

    #[test]
    fn closed_requires_beats_open_when() {
        let (rt, fs) = preview_state();
        let touched = changed_set(&["touched"]);
        let mut hook = hook(&["tool"], &[], Some(changed("touched")), vec![], 600);
        hook.requires = Some(changed("missing"));
        assert_eq!(
            preview_line(&hook, &rt, &fs, &touched).as_str(),
            "warn: tool cannot run (changed(missing))"
        );
    }

    #[test]
    fn closed_when_beats_passing_checks() {
        let (rt, fs) = preview_state();
        let touched = changed_set(&["touched"]);
        let mut hook = hook(
            &["tool"],
            &[],
            Some(changed("missing")),
            vec![Condition::Exists {
                path: "/opt/probe".into(),
            }],
            600,
        );
        hook.requires = Some(changed("touched"));
        assert_eq!(
            preview_line(&hook, &rt, &fs, &touched).as_str(),
            "skipped: tool (no need: changed(missing))"
        );
    }

    #[test]
    fn open_gates_with_passing_checks_skip_on_checks() {
        let (rt, fs) = preview_state();
        let touched = changed_set(&["touched"]);
        let mut hook = hook(
            &["tool"],
            &[],
            Some(changed("touched")),
            vec![Condition::Exists {
                path: "/opt/probe".into(),
            }],
            600,
        );
        hook.requires = Some(changed("touched"));
        assert_eq!(
            preview_line(&hook, &rt, &fs, &touched).as_str(),
            "skipped: tool (checks pass)"
        );
    }

    #[test]
    fn open_gates_with_failing_checks_run() {
        let (rt, fs) = preview_state();
        let touched = changed_set(&["touched"]);
        let mut hook = hook(
            &["tool"],
            &[],
            Some(changed("touched")),
            vec![Condition::Exists {
                path: "/opt/absent".into(),
            }],
            600,
        );
        hook.requires = Some(changed("touched"));
        assert_eq!(
            preview_line(&hook, &rt, &fs, &touched).as_str(),
            "! run: /opt/tool"
        );
    }

    #[test]
    fn added_hook_prints_plus_with_full_gates() {
        let mut added = hook(
            &["mise", "install"],
            &[],
            Some(changed("dest")),
            vec![in_path("probe")],
            60,
        );
        added.requires = Some(in_path("mise"));
        assert_eq!(
            lifecycle_lines(&[added], &[]),
            vec![
                "+ mise install".to_string(),
                "  + requires (in_path(mise))".to_string(),
                "  + when (changed(dest))".to_string(),
                "  + checks (in_path(probe))".to_string(),
            ]
        );
    }

    #[test]
    fn removed_hook_prints_bare_minus() {
        let mut recorded = hook(
            &["tool"],
            &[],
            Some(in_path("mise")),
            vec![in_path("probe")],
            60,
        );
        recorded.requires = Some(in_path("cap"));
        assert_eq!(
            lifecycle_lines(&[], &[recorded]),
            vec!["- tool".to_string()]
        );
    }

    #[test]
    fn modified_gate_prints_tilde_with_slot_diff() {
        let mut before = hook(&["mise", "install"], &[], None, vec![], 60);
        before.requires = Some(in_path("a"));
        let mut after = hook(&["mise", "install"], &[], None, vec![], 60);
        after.requires = Some(in_path("b"));
        assert_eq!(
            lifecycle_lines(&[after], &[before]),
            vec![
                "~ mise install".to_string(),
                "  ~ requires (in_path(a)) -> (in_path(b))".to_string(),
            ]
        );
    }

    #[test]
    fn unchanged_hook_prints_nothing() {
        let mut kept = hook(
            &["tool"],
            &[],
            Some(in_path("mise")),
            vec![in_path("probe")],
            60,
        );
        kept.requires = Some(in_path("cap"));
        assert!(lifecycle_lines(&[kept.clone()], &[kept]).is_empty());
    }

    #[test]
    fn empty_previous_prints_all_plus() {
        let current = vec![
            hook(&["tool", "first"], &[], None, vec![], 60),
            hook(&["tool", "second"], &[], None, vec![], 60),
        ];
        assert_eq!(
            lifecycle_lines(&current, &[]),
            vec!["+ tool first".to_string(), "+ tool second".to_string()]
        );
    }

    #[test]
    fn gate_absent_to_present_reads_added() {
        let before = hook(&["mise"], &[], None, vec![], 60);
        let after = hook(&["mise"], &[], Some(in_path("a")), vec![], 60);
        assert_eq!(
            lifecycle_lines(&[after], &[before]),
            vec!["~ mise".to_string(), "  + when (in_path(a))".to_string(),]
        );
    }

    #[test]
    fn gate_present_to_absent_reads_removed() {
        let before = hook(&["mise"], &[], Some(in_path("a")), vec![], 60);
        let after = hook(&["mise"], &[], None, vec![], 60);
        assert_eq!(
            lifecycle_lines(&[after], &[before]),
            vec!["~ mise".to_string(), "  - when (in_path(a))".to_string(),]
        );
    }

    #[test]
    fn gate_changed_single_reads_old_to_new() {
        let before = hook(&["mise"], &[], Some(in_path("a")), vec![], 60);
        let after = hook(&["mise"], &[], Some(in_path("b")), vec![], 60);
        assert_eq!(
            lifecycle_lines(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ when (in_path(a)) -> (in_path(b))".to_string(),
            ]
        );
    }

    #[test]
    fn checks_diff_reports_added_plus_removed() {
        let before = hook(&["mise"], &[], None, vec![in_path("a")], 60);
        let after = hook(&["mise"], &[], None, vec![in_path("b")], 60);
        assert_eq!(
            lifecycle_lines(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  - checks (in_path(a))".to_string(),
                "  + checks (in_path(b))".to_string(),
            ]
        );
    }

    #[test]
    fn timeout_change_reads_old_to_new() {
        let before = hook(&["mise"], &[], None, vec![], 60);
        let after = hook(&["mise"], &[], None, vec![], 600);
        assert_eq!(
            lifecycle_lines(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ timeout (60s -> 600s)".to_string(),
            ]
        );
    }
}
