//! Post-config steps that follow plans beside documents.

use lua_binding::Shape;
use lua_binding::error::{DecodeError, DecodeTag};
use lua_binding::value::Hooked;
use mlua::Lua;
use serde::{Deserialize, Serialize};

use crate::arg::Arg;
use crate::condition::Condition;
use crate::id::Id;

/// A hook run limit in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(transparent)]
#[shape(proxy = String)]
pub struct HookTimeout {
    /// The limit in seconds.
    secs: u64,
}

impl HookTimeout {
    /// Builds a hook limit from seconds.
    pub const fn new(secs: u64) -> Self {
        Self { secs }
    }

    /// Returns the limit in seconds.
    pub const fn secs(&self) -> u64 {
        self.secs
    }
}

/// The default hook run limit in seconds.
const DEFAULT_HOOK_TIMEOUT: u64 = 600;

impl Default for HookTimeout {
    fn default() -> Self {
        Self::new(DEFAULT_HOOK_TIMEOUT)
    }
}

impl From<u64> for HookTimeout {
    fn from(secs: u64) -> Self {
        Self::new(secs)
    }
}

impl Hooked<String> for HookTimeout {
    fn decode(_lua: &Lua, text: String) -> Result<Self, DecodeError> {
        parse_timeout(&text)
            .map(Self::new)
            .ok_or_else(|| DecodeError {
                segments: Vec::new(),
                reason: DecodeTag::ShapeMismatch {
                    require: "a duration like '10m'".to_owned(),
                    found: format!("'{text}'"),
                },
            })
    }

    fn encode(&self, _lua: &Lua) -> mlua::Result<String> {
        Ok(self.secs.to_string())
    }
}

/// A post-config step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(table, validate = reject_empty_argv)]
pub struct Hook {
    /// The command that the step runs.
    pub argv: Vec<Arg>,
    /// The PATH entries that only the subprocess sees.
    #[shape(default)]
    pub path: Vec<Arg>,
    /// The hook identifier.
    pub id: Id,
    /// The capability condition.
    #[serde(default)]
    #[shape(default)]
    pub requires: Condition,
    /// The need condition.
    #[serde(default)]
    #[shape(default)]
    pub when: Condition,
    /// The run condition.
    #[serde(default)]
    #[shape(default)]
    pub check: Condition,
    /// The run limit.
    #[shape(name = "timeout", default)]
    pub timeout_secs: HookTimeout,
}

/// One condition key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Shape)]
#[shape(table, str_alias)]
pub enum ConditionKey {
    /// The capability condition.
    Requires,
    /// The need condition.
    When,
    /// The run condition.
    Check,
}

/// One condition change.
#[derive(Debug, Clone, PartialEq, Eq, Shape)]
#[shape(table)]
pub struct ConditionChange {
    /// The condition slot that holds the change.
    pub key: ConditionKey,
    /// The recorded condition.
    pub before: Condition,
    /// The desired condition.
    pub after: Condition,
}

/// One timeout change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Shape)]
#[shape(table)]
pub struct TimeoutChange {
    /// The recorded limit in seconds.
    pub before: u64,
    /// The desired limit in seconds.
    pub after: u64,
}

/// The change to one hook.
#[derive(Debug, Clone, PartialEq, Eq, Shape)]
#[shape(table)]
pub struct HookModification {
    /// Condition edits in key order.
    pub conditions: Vec<ConditionChange>,
    /// The timeout edit.
    pub timeout: Option<TimeoutChange>,
}

/// One hook change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookChange {
    /// A hook with no recorded entry.
    Added,
    /// A hook with no desired entry.
    Removed,
    /// A hook whose conditions or timeout differ.
    Modified(HookModification),
    /// A hook that matches its recorded entry.
    Unchanged,
}

/// One hook and its change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookLifecycle<'a> {
    /// The hook that the lifecycle reports.
    pub hook: &'a Hook,
    /// The lifecycle change.
    pub change: HookChange,
}

impl HookModification {
    /// Returns whether the modification holds no edit.
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.timeout.is_none()
    }
}

impl HookChange {
    /// Returns whether the change adds the hook.
    pub fn is_added(&self) -> bool {
        matches!(self, Self::Added)
    }

    /// Returns whether the change removes the hook.
    pub fn is_removed(&self) -> bool {
        matches!(self, Self::Removed)
    }

    /// Returns whether the change carries a condition or timeout edit.
    pub fn is_modified(&self) -> bool {
        match self {
            Self::Modified(edits) => !edits.is_empty(),
            Self::Added | Self::Removed | Self::Unchanged => false,
        }
    }
}

impl HookLifecycle<'_> {
    /// Returns whether the lifecycle adds the hook.
    pub fn is_added(&self) -> bool {
        self.change.is_added()
    }

    /// Returns whether the lifecycle removes the hook.
    pub fn is_removed(&self) -> bool {
        self.change.is_removed()
    }

    /// Returns whether the lifecycle reports an edited hook.
    pub fn is_modified(&self) -> bool {
        self.change.is_modified()
    }
}

/// Pairs each desired and recorded hook with its change in render order.
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

/// Returns whether the ids match, or both argv and path match with no id.
fn same_identity(first: &Hook, second: &Hook) -> bool {
    if first.id.as_str().is_empty() || second.id.as_str().is_empty() {
        return first.argv == second.argv && first.path == second.path;
    }
    first.id == second.id
}

/// Returns the previous hook that shares identity with the desired hook.
fn find_recorded<'a>(hook: &Hook, previous: &'a [Hook]) -> Option<&'a Hook> {
    previous.iter().find(|held| same_identity(held, hook))
}

/// Returns the desired hook that shares identity with a recorded hook.
fn find_current<'a>(recorded: &Hook, current: &'a [Hook]) -> Option<&'a Hook> {
    current.iter().find(|hook| same_identity(hook, recorded))
}

/// Returns a condition change for a slot whose simplified conditions differ.
fn condition_change(
    key: ConditionKey,
    before: &Condition,
    after: &Condition,
) -> Option<ConditionChange> {
    if before.simplified() == after.simplified() {
        return None;
    }
    Some(ConditionChange {
        key,
        before: before.clone(),
        after: after.clone(),
    })
}

/// Returns a timeout change when the two caps differ.
fn timeout_change(before: u64, after: u64) -> Option<TimeoutChange> {
    if before == after {
        return None;
    }
    Some(TimeoutChange { before, after })
}

/// Collects every condition and timeout change between two hooks.
fn modify_hook(recorded: &Hook, hook: &Hook) -> HookModification {
    let mut conditions = Vec::new();
    if let Some(change) =
        condition_change(ConditionKey::Requires, &recorded.requires, &hook.requires)
    {
        conditions.push(change);
    }
    if let Some(change) = condition_change(ConditionKey::When, &recorded.when, &hook.when) {
        conditions.push(change);
    }
    if let Some(change) = condition_change(ConditionKey::Check, &recorded.check, &hook.check) {
        conditions.push(change);
    }
    HookModification {
        conditions,
        timeout: timeout_change(recorded.timeout_secs.secs(), hook.timeout_secs.secs()),
    }
}

/// Rejects a hook whose argv is empty.
///
/// # Errors
///
/// - [`mlua::Error`] when the argv is empty.
fn reject_empty_argv(hook: &Hook) -> mlua::Result<()> {
    if hook.argv.is_empty() {
        return Err(mlua::Error::external(
            "invalid hook argv 'table': require a non-empty string-or-route array",
        ));
    }
    Ok(())
}

/// Returns seconds from unit text like `10m`.
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
    use lua_binding::value::Decode;
    use mlua::Value;

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
            requires: Condition::default(),
            when,
            check,
            timeout_secs: HookTimeout::new(timeout_secs),
            id: Id::default(),
        }
    }

    fn in_path(name: &str) -> Condition {
        Condition::InPath {
            name: name.to_string(),
        }
    }

    #[test]
    fn lifecycle_marks_added_removed_unchanged() {
        let kept = hook(
            &["tool"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        let current = vec![
            kept.clone(),
            hook(
                &["fresh"],
                &[],
                Condition::default(),
                Condition::default(),
                60,
            ),
        ];
        let previous = vec![
            kept.clone(),
            hook(
                &["stale"],
                &[],
                Condition::default(),
                Condition::default(),
                60,
            ),
        ];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 3);
        assert!(matches!(lifecycle[0].change, HookChange::Unchanged));
        assert!(lifecycle[1].is_added());
        assert!(lifecycle[2].is_removed());
    }

    #[test]
    fn lifecycle_treats_equivalent_conditions_as_unchanged() {
        let mut before = hook(
            &["mise"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        before.requires = Condition::All(vec![in_path("mise"), in_path("mise")]);
        let mut after = hook(
            &["mise"],
            &[],
            Condition::default(),
            Condition::default(),
            60,
        );
        after.requires = in_path("mise");
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        assert!(matches!(lifecycle[0].change, HookChange::Unchanged));
    }

    #[test]
    fn lifecycle_reports_check_key_change() {
        let before = hook(&["mise"], &[], Condition::default(), in_path("a"), 60);
        let after = hook(&["mise"], &[], Condition::default(), in_path("b"), 60);
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        let edits = match &lifecycle[0].change {
            HookChange::Modified(edits) => edits,
            _ => panic!("expected a modified change"),
        };
        assert_eq!(edits.conditions.len(), 1);
        assert_eq!(edits.conditions[0].key, ConditionKey::Check);
        assert_eq!(edits.conditions[0].before, in_path("a"));
        assert_eq!(edits.conditions[0].after, in_path("b"));
    }

    #[test]
    fn lifecycle_reports_a_timeout_change() {
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
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        assert!(lifecycle[0].is_modified());
        let edits = match &lifecycle[0].change {
            HookChange::Modified(edits) => edits,
            _ => panic!("expected a modified change"),
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
    fn hook_decode_rejects_a_checks_field() {
        let lua = Lua::new();
        let value: Value = lua
            .load(r#"return { argv = { "mise" }, checks = { { in_path = { name = "x" } } } }"#)
            .call(())
            .unwrap();
        assert!(
            Hook::decode(&value, &lua).is_err(),
            "the hook decoder rejects a checks field"
        );
    }
}
