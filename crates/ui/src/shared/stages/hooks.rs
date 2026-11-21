//! Hook lifecycle blocks.

use confit_differ::view::View;
use confit_model::arg::{Arg, quote};
use confit_model::condition::Condition;
use confit_model::hook;
use confit_model::hook::{ConditionChange, ConditionKey, Hook, HookChange};
use extend::ext;

use crate::shared::components::{
    blocks::DisplayBlock,
    style::{Marks, Symbol},
};

#[ext(name = HookStage)]
pub(crate) impl View {
    /// Builds the hook lifecycle blocks.
    fn hooks(&self) -> Vec<DisplayBlock> {
        let mut out = Vec::new();
        for entry in hook::diff_lifecycle(&self.wanted.hooks, &self.recorded.hooks) {
            match &entry.change {
                HookChange::Added => {
                    out.push(hook_block(
                        Symbol::Add,
                        entry.hook,
                        added_branches(entry.hook),
                    ));
                }
                HookChange::Removed => {
                    out.push(hook_block(Symbol::Remove, entry.hook, Vec::new()));
                }
                HookChange::Modified(edits) => {
                    if edits.is_empty() {
                        continue;
                    }
                    let mut branches = changed_branches(&edits.conditions);
                    if let Some(timeout) = &edits.timeout {
                        branches.push(guard_branch(
                            Symbol::Update,
                            &format!("timeout ({}s -> {}s)", timeout.before, timeout.after),
                            &[],
                        ));
                    }
                    out.push(hook_block(Symbol::Update, entry.hook, branches));
                }
                HookChange::Unchanged => {}
            }
        }
        out
    }
}

/// Builds one hook branch with the argv mark on its root.
fn hook_block(symbol: Symbol, hook: &Hook, branches: Vec<DisplayBlock>) -> DisplayBlock {
    DisplayBlock::branch(Marks::plan(symbol), resolved_argv(&hook.argv), branches)
}

/// Returns the quoted argv with routes expanded to host paths.
fn resolved_argv(argv: &[Arg]) -> String {
    argv.iter()
        .map(|arg| match arg {
            Arg::Text(text) => text.clone(),
            Arg::Route(route) => route
                .resolve()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| route.display()),
        })
        .map(|text| quote(&text).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Returns one branch per guard leaf.
fn added_branches(hook: &Hook) -> Vec<DisplayBlock> {
    let mut out = Vec::new();
    for (key, condition) in [
        ("requires", &hook.requires),
        ("when", &hook.when),
        ("check", &hook.check),
    ] {
        if condition.is_true() {
            continue;
        }
        let (_, added) = Condition::default().leaf_diff(condition);
        if added.is_empty() {
            out.push(guard_branch(
                Symbol::Add,
                &format!("{key} ({})", condition.describe()),
                &[],
            ));
            continue;
        }
        let rows = added
            .iter()
            .map(|leaf| (Symbol::Add, leaf.describe()))
            .collect::<Vec<_>>();
        out.push(guard_branch(Symbol::Add, key, &rows));
    }
    out
}

/// Returns one branch per changed leaf.
fn changed_branches(conditions: &[ConditionChange]) -> Vec<DisplayBlock> {
    let mut out = Vec::new();
    for change in conditions {
        let key = condition_key_text(change.key);
        let (removed, added) = change.before.leaf_diff(&change.after);
        if removed.is_empty() && added.is_empty() {
            out.push(guard_branch(
                Symbol::Update,
                &format!(
                    "{key} ({}) -> ({})",
                    change.before.describe(),
                    change.after.describe()
                ),
                &[],
            ));
            continue;
        }
        let mut rows = Vec::new();
        for leaf in &removed {
            rows.push((Symbol::Remove, leaf.describe()));
        }
        for leaf in &added {
            rows.push((Symbol::Add, leaf.describe()));
        }
        out.push(guard_branch(Symbol::Update, key, &rows));
    }
    out
}

/// Returns the key text.
fn condition_key_text(key: ConditionKey) -> &'static str {
    match key {
        ConditionKey::Requires => "requires",
        ConditionKey::When => "when",
        ConditionKey::Check => "check",
    }
}

/// Builds one guard branch.
fn guard_branch(symbol: Symbol, label: &str, rows: &[(Symbol, String)]) -> DisplayBlock {
    DisplayBlock::branch(
        Marks::plan(symbol),
        label.to_owned(),
        rows.iter()
            .map(|(symbol, label)| DisplayBlock::leaf(Marks::plan(*symbol), label.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_differ::diff::Diff;
    use confit_model::arg::Arg;
    use confit_model::condition::Condition;
    use confit_model::hook::{Hook, HookTimeout};
    use confit_model::id::Id;
    use confit_model::manifest::Manifest;

    /// Builds one hook with a `when` guard.
    fn guarded_hook(when: Condition) -> Hook {
        Hook {
            argv: vec![Arg::Text("mise".to_owned())],
            path: Vec::new(),
            id: Id::default(),
            requires: Condition::default(),
            when,
            check: Condition::default(),
            timeout_secs: HookTimeout::new(60),
        }
    }

    #[test]
    fn added_hook_marks_key_branch_and_leaf_labels() {
        let desired = Manifest {
            version: [0, 9, 0],
            documents: Vec::new(),
            hooks: vec![guarded_hook(Condition::All(vec![
                Condition::InPath {
                    name: "git".to_owned(),
                },
                Condition::InPath {
                    name: "curl".to_owned(),
                },
            ]))],
        };
        let previous = Manifest {
            version: [0, 9, 0],
            documents: Vec::new(),
            hooks: Vec::new(),
        };
        let diff = Diff::build(desired, previous).unwrap();
        let found = diff.desired.hooks();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "mise");
        assert_eq!(found[0].mark.symbol, Some(Symbol::Add));
        assert_eq!(found[0].children.len(), 1);
        assert_eq!(found[0].children[0].text, "when");
        assert_eq!(found[0].children[0].mark.symbol, Some(Symbol::Add));
        assert_eq!(found[0].children[0].children.len(), 2);
        let mut labels = found[0].children[0]
            .children
            .iter()
            .map(|leaf| leaf.text.as_str())
            .collect::<Vec<_>>();
        labels.sort_unstable();
        assert_eq!(labels, vec!["in_path(curl)", "in_path(git)"]);
    }
}
