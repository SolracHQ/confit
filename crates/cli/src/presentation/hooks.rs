//! Hooks
//! Hook lifecycle lines for plans beside documents.

use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::hook::{GateChange, GateSlot, Hook, HookChange, HookLifecycle};

use crate::presentation::summary::Sigil;

/// Render one condition in compact form for gate text.
pub fn describe_condition(cond: &Condition) -> String {
    fn render(cond: &Condition) -> String {
        match cond {
            Condition::EnvEq { key, value } => format!("env_eq({key}={value})"),
            Condition::EnvSet { key } => format!("env_set({key})"),
            Condition::InPath { name } => format!("in_path({name})"),
            Condition::Exists { route } => format!("exists({})", route.display()),
            Condition::Changed { route } => format!("changed({})", route.display()),
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

/// Render plan hook lines from lifecycle diff data.
pub fn lifecycle_lines(lifecycle: &[HookLifecycle]) -> Vec<String> {
    let mut lines = Vec::new();
    for entry in lifecycle {
        match &entry.change {
            HookChange::Added => {
                lines.push(format!(
                    "{} {}",
                    Sigil::Add.mark(),
                    Arg::join(&entry.hook.argv)
                ));
                full_gates(entry.hook, &mut lines);
            }
            HookChange::Removed => {
                lines.push(format!(
                    "{} {}",
                    Sigil::Remove.mark(),
                    Arg::join(&entry.hook.argv)
                ));
            }
            HookChange::Modified(edits) => {
                if edits.is_empty() {
                    continue;
                }
                lines.push(format!(
                    "{} {}",
                    Sigil::Update.mark(),
                    Arg::join(&entry.hook.argv)
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

/// Render full gate lines for one added hook.
///
/// - Requires and when print once while present, checks print once per member,
///   all marked new in slot order.
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

/// Return one gate slot name for detail lines.
fn gate_slot_text(slot: GateSlot) -> &'static str {
    match slot {
        GateSlot::Requires => "requires",
        GateSlot::When => "when",
    }
}

/// Render one gate change line per slot while text differs.
///
/// - Absent-to-present is added, present-to-absent is removed, changed
///   is old-to-new.
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

/// Render removed check lines before added check lines.
///
/// - Removals print in previous order, additions print in desired order.
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

/// Render one timeout with the run cap suffix.
fn timeout_text(secs: u64) -> String {
    format!("{secs}s")
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::hook::merge_hooks;

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

    fn render_lifecycle(current: &[Hook], previous: &[Hook]) -> Vec<String> {
        lifecycle_lines(&confit_model::hook::diff_lifecycle(current, previous))
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
            route: confit_model::routes::Route::new(confit_model::routes::RouteBase::Literal, path)
                .unwrap(),
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
}
