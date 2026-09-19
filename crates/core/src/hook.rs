//! Hook
//!
//! Post-config steps riding plans beside documents.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::document::Condition;
use crate::fs::Filesystem;
use crate::runtime::{Runtime, evaluate, find_binary};

/// One post-config step with gates plus checks.
///
/// Argv executes directly with no shell in between. Path
/// extends PATH for the hook subprocess alone. When gates
/// the run on runtime conditions. Checks prove the run with
/// the same shapes: passing checks skip the hook, failing
/// checks run it. Timeout caps the run in seconds.
///
/// # Examples
///
/// ```rust
/// use confit_core::hook::Hook;
///
/// let hook = Hook {
///     argv: vec!["mise".into(), "install".into()],
///     path: Vec::new(),
///     when: None,
///     checks: Vec::new(),
///     timeout_secs: 600,
/// };
/// assert!(matches!(hook.argv.len(), 2));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    /// Holds the command plus arguments in order.
    pub argv: Vec<String>,
    /// Holds the PATH extension dirs for the subprocess alone.
    pub path: Vec<String>,
    /// Holds the run gate. None runs unconditionally.
    pub when: Option<Condition>,
    /// Holds the proof conditions, AND by list.
    pub checks: Vec<Condition>,
    /// Holds the run cap in seconds.
    pub timeout_secs: u64,
}

/// Merges hooks sharing argv plus path into one run each.
///
/// First-seen order wins. Gates join with OR: `Any` of the
/// two sides, flattening nested `Any`. An ungated side keeps
/// the merged hook ungated. Checks concatenate. Timeout
/// takes the max. Single hooks pass through untouched, so
/// the common case never grows `Any` wrappers.
///
/// # Arguments
///
/// * `hooks` - the declared hooks in declaration order.
///
/// # Returns
///
/// The merged hooks in first-seen order.
///
/// # Examples
///
/// ```rust
/// use confit_core::hook::{Hook, merge_hooks};
///
/// let hook = Hook {
///     argv: vec!["mise".into()],
///     path: Vec::new(),
///     when: None,
///     checks: Vec::new(),
///     timeout_secs: 60,
/// };
/// assert!(matches!(merge_hooks(vec![hook]).len(), 1));
/// ```
pub fn merge_hooks(hooks: Vec<Hook>) -> Vec<Hook> {
    let mut order: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    let mut merged: Vec<Hook> = Vec::new();
    for hook in hooks {
        let key = (hook.argv.clone(), hook.path.clone());
        match order.iter().position(|held| *held == key) {
            Some(index) => {
                let current = &mut merged[index];
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
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::hook::{Hook, resolve_hook};
/// use confit_core::runtime::Runtime;
///
/// let hook = Hook {
///     argv: vec!["mise".into()],
///     path: Vec::new(),
///     when: None,
///     checks: Vec::new(),
///     timeout_secs: 600,
/// };
/// let rt = Runtime { vars: Default::default(), path_dirs: Vec::new() };
/// assert!(matches!(resolve_hook(&hook, &rt, &MemoryFs::new()), None));
/// ```
pub fn resolve_hook(hook: &Hook, rt: &Runtime, fs: &dyn Filesystem) -> Option<PathBuf> {
    let head = hook.argv.first()?;
    let mut dirs: Vec<PathBuf> = hook.path.iter().map(PathBuf::from).collect();
    dirs.extend(rt.path_dirs.iter().cloned());
    find_binary(head, &dirs, fs)
}

/// Reports whether one hook reads satisfied with passing checks.
fn checks_pass(hook: &Hook, rt: &Runtime, fs: &dyn Filesystem) -> bool {
    !hook.checks.is_empty() && hook.checks.iter().all(|check| evaluate(check, rt, fs))
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
/// Closed gates warn with the gate named. Satisfied checks
/// skip with the work standing done. Everything else resolves
/// the binary and runs.
///
/// # Arguments
///
/// * `hook` - the hook under preview.
/// * `rt` - the runtime facts under reading.
/// * `fs` - the backend under stating.
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
) -> crate::error::Result<String> {
    if let Some(gate) = hook.when.as_ref()
        && !evaluate(gate, rt, fs)
    {
        return Ok(format!(
            "warn: {} cannot run ({})",
            argv_text(hook),
            describe_condition(gate)
        ));
    }
    if checks_pass(hook, rt, fs) {
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
/// # Arguments
///
/// * `cond` - the condition under rendering.
///
/// # Returns
///
/// The compact gate text like `in_path(mise)`.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::Condition;
/// use confit_core::hook::describe_condition;
///
/// let cond = Condition::InPath { name: "mise".into() };
/// assert!(matches!(describe_condition(&cond).as_str(), "in_path(mise)"));
/// ```
pub fn describe_condition(cond: &Condition) -> String {
    match cond {
        Condition::EnvEq { key, value } => format!("env_eq({key}={value})"),
        Condition::EnvSet { key } => format!("env_set({key})"),
        Condition::InPath { name } => format!("in_path({name})"),
        Condition::Exists { path } => format!("exists({path})"),
        Condition::All(items) => format!(
            "all({})",
            items
                .iter()
                .map(describe_condition)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Condition::Any(items) => format!(
            "any({})",
            items
                .iter()
                .map(describe_condition)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Condition::Not(inner) => format!("nop({})", describe_condition(inner)),
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
}
