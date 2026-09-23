//! Hooks
//!
//! Hook preview lines for plans beside documents.

use confit_core::condition::Condition;
use confit_core::error::Result;
use confit_core::hook::{GateChange, GateSlot, Hook, HookChange, HookLifecycle, resolve_hook};
use confit_core::ids::DocPath;
use confit_core::plan::Bundle;
use confit_core::probe::PathProbe;
use confit_core::runtime::Runtime;
use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::presentation::summary::Sigil;

/// One hook preview outcome behind one evaluated line.
///
/// Gate text renders through condition collapse, so merged
/// duplicate gates read once. Binaries resolve before running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewOutcome {
    /// Requires gate closed with its rendered text.
    RequiresClosed(String),
    /// When gate closed with its rendered text.
    WhenClosed(String),
    /// Checks all pass with the work standing done.
    ChecksPass,
    /// Hook runs with its resolved binary.
    Run(PathBuf),
}

/// One evaluated hook pairing its hook with its preview outcome.
///
/// The hook borrows core data, the outcome carries presentation
/// facts. Rendering reads argv plus the outcome alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluatedHook<'a> {
    /// Holds the hook under preview.
    pub hook: &'a Hook,
    /// Holds the gate or binary behind its line.
    pub outcome: PreviewOutcome,
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
/// use confit_cli::presentation::hooks::describe_condition;
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

/// Renders plan hook lines from lifecycle diff data.
///
/// # Arguments
///
/// * `lifecycle` - the lifecycle entries in plan order with removals trailing.
///
/// # Returns
///
/// The lifecycle lines in plan order with removals trailing.
///
/// # Examples
///
/// ```rust
/// use confit_core::hook::{Hook, diff_lifecycle};
/// use confit_cli::presentation::hooks::lifecycle_lines;
///
/// let hook = Hook {
///     argv: vec!["mise".to_string()],
///     path: vec![],
///     requires: None,
///     when: None,
///     checks: vec![],
///     timeout_secs: 60,
/// };
/// let current = [hook];
/// let lifecycle = diff_lifecycle(&current, &[]);
/// assert_eq!(lifecycle_lines(&lifecycle), vec!["+ mise".to_string()]);
/// ```
pub fn lifecycle_lines(lifecycle: &[HookLifecycle]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in lifecycle {
        match &entry.change {
            HookChange::Added => {
                lines.push(format!("{} {}", Sigil::Add.mark(), argv_text(entry.hook)));
                full_gates(entry.hook, &mut lines);
            }
            HookChange::Removed => {
                lines.push(format!(
                    "{} {}",
                    Sigil::Remove.mark(),
                    argv_text(entry.hook)
                ));
            }
            HookChange::Modified(edits) => {
                if edits.is_empty() {
                    continue;
                }
                lines.push(format!(
                    "{} {}",
                    Sigil::Update.mark(),
                    argv_text(entry.hook)
                ));
                render_gate_changes(&edits.gates, &mut lines);
                render_check_changes(&edits.removed_checks, &edits.added_checks, &mut lines);
                if let Some(timeout) = &edits.timeout {
                    lines.push(format!(
                        "  {} timeout ({} -> {})",
                        Sigil::Update.mark(),
                        timeout_text(timeout.before),
                        timeout_text(timeout.after)
                    ));
                }
            }
            HookChange::Unchanged => {}
        }
    }
    lines
}

/// Reads one hook preview outcome.
///
/// First closed mouth speaks: requires, when, checks, then
/// resolve.
///
/// # Errors
///
/// Unresolvable binaries fail as plan errors naming the hook.
fn decide<'a>(
    hook: &'a Hook,
    rt: &Runtime,
    probe: &dyn PathProbe,
    changed: &BTreeSet<DocPath>,
) -> Result<EvaluatedHook<'a>> {
    if let Some(gate) = hook.requires.as_ref()
        && !rt.evaluate(gate, probe, changed)
    {
        return Ok(EvaluatedHook {
            hook,
            outcome: PreviewOutcome::RequiresClosed(describe_condition(gate)),
        });
    }
    if let Some(gate) = hook.when.as_ref()
        && !rt.evaluate(gate, probe, changed)
    {
        return Ok(EvaluatedHook {
            hook,
            outcome: PreviewOutcome::WhenClosed(describe_condition(gate)),
        });
    }
    if checks_pass(hook, rt, probe, changed) {
        return Ok(EvaluatedHook {
            hook,
            outcome: PreviewOutcome::ChecksPass,
        });
    }
    let Some(binary) = resolve_hook(hook, rt, probe) else {
        let head = hook.argv.first().cloned().unwrap_or_default();
        return Err(confit_core::error::Error::Plan(format!(
            "hook '{}' cannot resolve '{head}'",
            argv_text(hook)
        )));
    };
    Ok(EvaluatedHook {
        hook,
        outcome: PreviewOutcome::Run(binary),
    })
}

/// Evaluates one preview outcome per hook in plan order.
///
/// # Arguments
///
/// * `bundle` - the bundle holding hooks under preview.
/// * `rt` - the runtime facts under reading.
/// * `probe` - the probe under stating.
/// * `changed` - the changed document ids under reading.
///
/// # Returns
///
/// The evaluated hooks in plan order.
///
/// # Errors
///
/// Unresolvable binaries fail as plan errors naming the hook.
pub fn evaluate_hooks<'a>(
    bundle: &'a Bundle,
    rt: &Runtime,
    probe: &dyn PathProbe,
    changed: &BTreeSet<DocPath>,
) -> Result<Vec<EvaluatedHook<'a>>> {
    bundle
        .manifest
        .hooks
        .iter()
        .map(|hook| decide(hook, rt, probe, changed))
        .collect()
}

/// Renders one evaluated hook line.
fn render_preview(evaluated: &EvaluatedHook) -> String {
    let argv = argv_text(evaluated.hook);
    match &evaluated.outcome {
        PreviewOutcome::RequiresClosed(gate) => {
            format!("warn: {argv} cannot run ({gate})")
        }
        PreviewOutcome::WhenClosed(gate) => {
            format!("skipped: {argv} (no need: {gate})")
        }
        PreviewOutcome::ChecksPass => format!("skipped: {argv} (checks pass)"),
        PreviewOutcome::Run(binary) => run_line(evaluated.hook, binary),
    }
}

/// Renders one evaluated line per hook in plan order.
///
/// # Arguments
///
/// * `evaluated` - the evaluated hooks under rendering.
///
/// # Returns
///
/// The preview lines in plan order.
pub fn render_evaluated(evaluated: &[EvaluatedHook]) -> Vec<String> {
    evaluated.iter().map(render_preview).collect()
}

/// Reports whether one hook reads satisfied with passing checks.
fn checks_pass(
    hook: &Hook,
    rt: &Runtime,
    probe: &dyn PathProbe,
    changed: &BTreeSet<DocPath>,
) -> bool {
    !hook.checks.is_empty()
        && hook
            .checks
            .iter()
            .all(|check| rt.evaluate(check, probe, changed))
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

/// Renders full gate lines for one added hook.
///
/// Requires and when print once while present, checks print
/// once per member, all marked new in slot order.
fn full_gates(hook: &Hook, out: &mut Vec<String>) {
    if let Some(gate) = hook.requires.as_ref() {
        out.push(format!(
            "  {} requires ({})",
            Sigil::Add.mark(),
            describe_condition(gate)
        ));
    }
    if let Some(gate) = hook.when.as_ref() {
        out.push(format!(
            "  {} when ({})",
            Sigil::Add.mark(),
            describe_condition(gate)
        ));
    }
    for check in &hook.checks {
        out.push(format!(
            "  {} checks ({})",
            Sigil::Add.mark(),
            describe_condition(check)
        ));
    }
}

/// Reads one gate slot name for detail lines.
fn gate_slot_text(slot: GateSlot) -> &'static str {
    match slot {
        GateSlot::Requires => "requires",
        GateSlot::When => "when",
    }
}

/// Renders one gate change line per slot while text differs.
///
/// Absent-to-present reads added, present-to-absent reads
/// removed, changed reads old-to-new. Equal describe text
/// never reaches this path.
fn render_gate_changes(gates: &[GateChange], out: &mut Vec<String>) {
    for change in gates {
        let slot = gate_slot_text(change.slot);
        let prior = change.before.as_ref().map(describe_condition);
        let next = change.after.as_ref().map(describe_condition);
        match (prior, next) {
            (Some(before), Some(after)) => {
                out.push(format!(
                    "  {} {slot} ({before}) -> ({after})",
                    Sigil::Update.mark()
                ));
            }
            (None, Some(after)) => {
                out.push(format!("  {} {slot} ({after})", Sigil::Add.mark()));
            }
            (Some(before), None) => {
                out.push(format!("  {} {slot} ({before})", Sigil::Remove.mark()));
            }
            (None, None) => {}
        }
    }
}

/// Renders removed check lines before added check lines.
///
/// Members arrive split by core, so rendering reads order
/// alone. Removals print in previous order, additions print
/// in desired order.
fn render_check_changes(removed: &[Condition], added: &[Condition], out: &mut Vec<String>) {
    for check in removed {
        out.push(format!(
            "  {} checks ({})",
            Sigil::Remove.mark(),
            describe_condition(check)
        ));
    }
    for check in added {
        out.push(format!(
            "  {} checks ({})",
            Sigil::Add.mark(),
            describe_condition(check)
        ));
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
    use confit_core::hook::merge_hooks;

    fn preview_hook(
        hook: &Hook,
        rt: &Runtime,
        probe: &dyn PathProbe,
        changed: &BTreeSet<DocPath>,
    ) -> Result<String> {
        Ok(render_preview(&decide(hook, rt, probe, changed)?))
    }

    fn hook_preview(
        bundle: &Bundle,
        rt: &Runtime,
        probe: &dyn PathProbe,
        changed: &BTreeSet<DocPath>,
    ) -> Result<Vec<String>> {
        Ok(render_evaluated(&evaluate_hooks(
            bundle, rt, probe, changed,
        )?))
    }

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

    fn render_lifecycle(current: &[Hook], previous: &[Hook]) -> Vec<String> {
        lifecycle_lines(&confit_core::hook::diff_lifecycle(current, previous))
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

    fn preview_state() -> (Runtime, confit_core::probe::MemoryProbe) {
        let mut probe = confit_core::probe::MemoryProbe::new();
        probe.exec(std::path::Path::new("/opt/tool"));
        probe.file(std::path::Path::new("/opt/probe"));
        let rt = Runtime {
            vars: Default::default(),
            path_dirs: vec![std::path::PathBuf::from("/opt")],
        };
        (rt, probe)
    }

    fn changed_set(dests: &[&str]) -> BTreeSet<DocPath> {
        dests.iter().copied().map(DocPath::new).collect()
    }

    fn preview_line(
        hook: &Hook,
        rt: &Runtime,
        probe: &dyn PathProbe,
        changed: &BTreeSet<DocPath>,
    ) -> String {
        match preview_hook(hook, rt, probe, changed) {
            Ok(line) => line,
            Err(error) => panic!("preview renders: {error}"),
        }
    }

    #[test]
    fn closed_requires_beats_open_when() {
        let (rt, probe) = preview_state();
        let touched = changed_set(&["touched"]);
        let mut hook = hook(&["tool"], &[], Some(changed("touched")), vec![], 600);
        hook.requires = Some(changed("missing"));
        assert_eq!(
            preview_line(&hook, &rt, &probe, &touched).as_str(),
            "warn: tool cannot run (changed(missing))"
        );
    }

    #[test]
    fn closed_when_beats_passing_checks() {
        let (rt, probe) = preview_state();
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
            preview_line(&hook, &rt, &probe, &touched).as_str(),
            "skipped: tool (no need: changed(missing))"
        );
    }

    #[test]
    fn open_gates_with_passing_checks_skip_on_checks() {
        let (rt, probe) = preview_state();
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
            preview_line(&hook, &rt, &probe, &touched).as_str(),
            "skipped: tool (checks pass)"
        );
    }

    #[test]
    fn open_gates_with_failing_checks_run() {
        let (rt, probe) = preview_state();
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
            preview_line(&hook, &rt, &probe, &touched).as_str(),
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
            render_lifecycle(&[added], &[]),
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
            render_lifecycle(&[], &[recorded]),
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
            render_lifecycle(&[after], &[before]),
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
        assert!(render_lifecycle(&[kept.clone()], &[kept]).is_empty());
    }

    #[test]
    fn empty_previous_prints_all_plus() {
        let current = vec![
            hook(&["tool", "first"], &[], None, vec![], 60),
            hook(&["tool", "second"], &[], None, vec![], 60),
        ];
        assert_eq!(
            render_lifecycle(&current, &[]),
            vec!["+ tool first".to_string(), "+ tool second".to_string()]
        );
    }

    #[test]
    fn gate_absent_to_present_reads_added() {
        let before = hook(&["mise"], &[], None, vec![], 60);
        let after = hook(&["mise"], &[], Some(in_path("a")), vec![], 60);
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec!["~ mise".to_string(), "  + when (in_path(a))".to_string(),]
        );
    }

    #[test]
    fn gate_present_to_absent_reads_removed() {
        let before = hook(&["mise"], &[], Some(in_path("a")), vec![], 60);
        let after = hook(&["mise"], &[], None, vec![], 60);
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec!["~ mise".to_string(), "  - when (in_path(a))".to_string(),]
        );
    }

    #[test]
    fn gate_changed_single_reads_old_to_new() {
        let before = hook(&["mise"], &[], Some(in_path("a")), vec![], 60);
        let after = hook(&["mise"], &[], Some(in_path("b")), vec![], 60);
        assert_eq!(
            render_lifecycle(&[after], &[before]),
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
            render_lifecycle(&[after], &[before]),
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
            render_lifecycle(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ timeout (60s -> 600s)".to_string(),
            ]
        );
    }

    fn preview_runtime() -> (
        confit_core::runtime::Runtime,
        confit_core::probe::MemoryProbe,
    ) {
        let mut probe = confit_core::probe::MemoryProbe::new();
        probe.exec(std::path::Path::new("/opt/tool"));
        probe.file(std::path::Path::new("/opt/probe"));
        let rt = confit_core::runtime::Runtime {
            vars: std::collections::BTreeMap::new(),
            path_dirs: vec![std::path::PathBuf::from("/opt")],
        };
        (rt, probe)
    }

    fn hook_fixture(argv: &[&str]) -> confit_core::hook::Hook {
        confit_core::hook::Hook {
            argv: argv.iter().map(|item| item.to_string()).collect(),
            path: Vec::new(),
            requires: None,
            when: None,
            checks: Vec::new(),
            timeout_secs: confit_core::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }
    }

    #[test]
    fn hook_preview_renders_run_skip_warn_lines() {
        use confit_core::condition::Condition;

        let (rt, probe) = preview_runtime();
        let bundle = Bundle {
            manifest: confit_core::store::manifest::Manifest {
                version: confit_core::plan::BUNDLE_VERSION,
                documents: Vec::new(),
                hooks: vec![
                    hook_fixture(&["tool", "--flag"]),
                    confit_core::hook::Hook {
                        checks: vec![Condition::Exists {
                            path: "/opt/probe".into(),
                        }],
                        ..hook_fixture(&["tool"])
                    },
                    confit_core::hook::Hook {
                        when: Some(Condition::InPath {
                            name: "absent".into(),
                        }),
                        ..hook_fixture(&["tool"])
                    },
                ],
            },
            blobs: std::collections::BTreeMap::new(),
        };
        let lines = match hook_preview(&bundle, &rt, &probe, &BTreeSet::new()) {
            Ok(lines) => lines,
            Err(error) => panic!("preview renders: {error}"),
        };
        assert_eq!(
            lines,
            vec![
                "! run: /opt/tool --flag".to_string(),
                "skipped: tool (checks pass)".to_string(),
                "skipped: tool (no need: in_path(absent))".to_string(),
            ]
        );
    }

    #[test]
    fn hook_preview_runs_on_failing_checks() {
        use confit_core::condition::Condition;

        let (rt, probe) = preview_runtime();
        let bundle = Bundle {
            manifest: confit_core::store::manifest::Manifest {
                version: confit_core::plan::BUNDLE_VERSION,
                documents: Vec::new(),
                hooks: vec![confit_core::hook::Hook {
                    checks: vec![Condition::Exists {
                        path: "/opt/absent".into(),
                    }],
                    ..hook_fixture(&["tool"])
                }],
            },
            blobs: std::collections::BTreeMap::new(),
        };
        let lines = match hook_preview(&bundle, &rt, &probe, &BTreeSet::new()) {
            Ok(lines) => lines,
            Err(error) => panic!("preview renders: {error}"),
        };
        assert_eq!(lines, vec!["! run: /opt/tool".to_string()]);
    }

    #[test]
    fn hook_preview_miss_fails_naming_hook() {
        let (rt, probe) = preview_runtime();
        let bundle = Bundle {
            manifest: confit_core::store::manifest::Manifest {
                version: confit_core::plan::BUNDLE_VERSION,
                documents: Vec::new(),
                hooks: vec![hook_fixture(&["absent", "install"])],
            },
            blobs: std::collections::BTreeMap::new(),
        };
        match hook_preview(&bundle, &rt, &probe, &BTreeSet::new()) {
            Ok(_) => panic!("missing binary passes"),
            Err(error) => assert_eq!(
                error.to_string(),
                "hook 'absent install' cannot resolve 'absent'"
            ),
        }
    }
}
