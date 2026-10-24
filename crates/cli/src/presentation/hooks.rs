//! Hooks
//! Hook lifecycle lines for plans beside documents.

use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::hook::{ConditionChange, ConditionKey, Hook, HookChange, HookLifecycle};
use confit_runtime::Unmet;

use crate::presentation::summary::Sigil;

/// Render one condition in compact form for condition text.
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

/// Render one unmet condition with its failing member.
pub fn describe_unmet(fault: &Unmet) -> String {
    let head = describe_condition(&fault.member);
    if fault.cause.is_empty() {
        return head;
    }
    let tail = fault
        .cause
        .iter()
        .map(describe_unmet)
        .collect::<Vec<_>>()
        .join(", ");
    if matches!(fault.member, Condition::Any(_)) {
        return format!("{head} [{tail}]");
    }
    format!("{head} [{}: {tail}]", fault.index)
}

/// Render one blocked condition with its fault beside the words.
pub fn describe_blocked(condition: &Condition, fault: &Unmet) -> String {
    if fault.member == *condition {
        return describe_unmet(fault);
    }
    format!(
        "{} [{}]",
        describe_condition(condition),
        describe_unmet(fault)
    )
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
                full_conditions(entry.hook, &mut lines);
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
                render_condition_changes(&edits.conditions, &mut lines);
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

/// Render full condition lines for one added hook.
///
/// - True conditions stay silent.
fn full_conditions(hook: &Hook, out: &mut Vec<String>) {
    for (key, condition) in [
        ("requires", &hook.requires),
        ("when", &hook.when),
        ("check", &hook.check),
    ] {
        if condition.is_true() {
            continue;
        }
        out.push(format!(
            "  {} {key} ({})",
            Sigil::Add.mark(),
            describe_condition(condition)
        ));
    }
}

/// Return one condition key name for detail lines.
fn condition_key_text(key: ConditionKey) -> &'static str {
    match key {
        ConditionKey::Requires => "requires",
        ConditionKey::When => "when",
        ConditionKey::Check => "check",
    }
}

/// Render one condition change line per key while text differs.
fn render_condition_changes(conditions: &[ConditionChange], out: &mut Vec<String>) {
    for change in conditions {
        let key = condition_key_text(change.key);
        out.push(format!(
            "  {} {key} ({}) -> ({})",
            Sigil::Update.mark(),
            describe_condition(&change.before),
            describe_condition(&change.after)
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
    use confit_model::hook::diff_lifecycle;
    use confit_model::routes::{Route, RouteBase};

    fn hook(
        argv: &[&str],
        path: &[&str],
        when: Condition,
        check: Condition,
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
            id: None,
            requires: Condition::default(),
            when,
            check,
            timeout_secs,
        }
    }

    fn in_path(name: &str) -> Condition {
        Condition::InPath {
            name: name.to_string(),
        }
    }

    fn render_lifecycle(current: &[Hook], previous: &[Hook]) -> Vec<String> {
        lifecycle_lines(&diff_lifecycle(current, previous))
    }

    fn changed(path: &str) -> Condition {
        Condition::Changed {
            route: Route::new(RouteBase::Literal, path).unwrap(),
        }
    }

    #[test]
    fn describe_collapses_merged_duplicate_conditions() {
        let condition =
            || Condition::All(vec![in_path("mise"), changed("~/.config/mise/config.toml")]);
        let merged = Condition::Any(vec![
            condition(),
            condition(),
            condition(),
            condition(),
            condition(),
        ]);
        assert_eq!(
            describe_condition(&merged).as_str(),
            "(in_path(mise) and changed(~/.config/mise/config.toml))"
        );
    }

    #[test]
    fn describe_nests_compounds_with_full_parens() {
        let condition = Condition::Any(vec![
            Condition::All(vec![in_path("a"), in_path("b")]),
            Condition::Not(Box::new(in_path("c"))),
        ]);
        assert_eq!(
            describe_condition(&condition).as_str(),
            "((in_path(a) and in_path(b)) or (not in_path(c)))"
        );
    }

    #[test]
    fn added_hook_prints_plus_with_full_conditions() {
        let mut added = hook(
            &["mise", "install"],
            &[],
            changed("dest"),
            in_path("probe"),
            60,
        );
        added.requires = in_path("mise");
        assert_eq!(
            render_lifecycle(&[added], &[]),
            vec![
                "+ mise install".to_string(),
                "  + requires (in_path(mise))".to_string(),
                "  + when (changed(dest))".to_string(),
                "  + check (in_path(probe))".to_string(),
            ]
        );
    }

    #[test]
    fn removed_hook_prints_bare_minus() {
        let mut recorded = hook(&["tool"], &[], in_path("mise"), in_path("probe"), 60);
        recorded.requires = in_path("cap");
        assert_eq!(
            render_lifecycle(&[], &[recorded]),
            vec!["- tool".to_string()]
        );
    }

    #[test]
    fn modified_condition_prints_tilde_with_key_diff() {
        let mut before = hook(
            &["mise", "install"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        before.requires = in_path("a");
        let mut after = hook(
            &["mise", "install"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        after.requires = in_path("b");
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec![
                "~ mise install".to_string(),
                "  ~ requires (in_path(a)) -> (in_path(b))".to_string(),
            ]
        );
    }

    #[test]
    fn empty_previous_prints_all_plus() {
        let current = vec![
            hook(
                &["tool", "first"],
                &[],
                Condition::default(),
                Condition::default(),
                60,
            ),
            hook(
                &["tool", "second"],
                &[],
                Condition::default(),
                Condition::default(),
                60,
            ),
        ];
        assert_eq!(
            render_lifecycle(&current, &[]),
            vec!["+ tool first".to_string(), "+ tool second".to_string()]
        );
    }

    #[test]
    fn condition_changes_read_old_to_new() {
        let before = hook(
            &["mise"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        let after = hook(&["mise"], &[], in_path("a"), Condition::default(), 60);
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ when (true) -> (in_path(a))".to_string(),
            ]
        );
        let before = hook(&["mise"], &[], in_path("a"), Condition::default(), 60);
        let after = hook(&["mise"], &[], in_path("b"), Condition::default(), 60);
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ when (in_path(a)) -> (in_path(b))".to_string(),
            ]
        );
    }

    #[test]
    fn check_diff_reports_key_change() {
        let before = hook(&["mise"], &[], Condition::default(), in_path("a"), 60);
        let after = hook(&["mise"], &[], Condition::default(), in_path("b"), 60);
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ check (in_path(a)) -> (in_path(b))".to_string(),
            ]
        );
    }

    #[test]
    fn timeout_change_parsed_old_to_new() {
        let before = hook(
            &["mise"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        let after = hook(
            &["mise"],
            &[],
            Condition::default(),
            Condition::default(),
            600,
        );
        assert_eq!(
            render_lifecycle(&[after], &[before]),
            vec![
                "~ mise".to_string(),
                "  ~ timeout (60s -> 600s)".to_string(),
            ]
        );
    }

    #[test]
    fn unmet_leaf_parsed_member_words() {
        let fault = Unmet {
            index: 0,
            member: in_path("mise"),
            cause: Vec::new(),
        };
        assert_eq!(describe_unmet(&fault).as_str(), "in_path(mise)");
        assert_eq!(
            describe_blocked(&in_path("mise"), &fault).as_str(),
            "in_path(mise)"
        );
    }

    #[test]
    fn unmet_conjunction_names_failing_member() {
        let fault = Unmet {
            index: 1,
            member: in_path("b"),
            cause: Vec::new(),
        };
        let condition = Condition::All(vec![in_path("a"), in_path("b")]);
        assert_eq!(describe_unmet(&fault).as_str(), "in_path(b)");
        assert_eq!(
            describe_blocked(&condition, &fault).as_str(),
            "(in_path(a) and in_path(b)) [in_path(b)]"
        );
    }
}
