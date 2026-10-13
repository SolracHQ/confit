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
    /// The hook identifier.
    pub id: Option<String>,
    /// The capability condition.
    #[serde(default)]
    #[face(default)]
    pub requires: Condition,
    /// The need condition.
    #[serde(default)]
    #[face(default)]
    pub when: Condition,
    /// The run condition.
    #[serde(default)]
    #[face(default)]
    pub check: Condition,
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
    /// The run condition.
    Check,
}

/// One condition change.
#[derive(Debug, Clone, PartialEq, Eq, Face)]
#[face(table)]
pub struct ConditionChange {
    /// The slot under diffing.
    pub slot: ConditionSlot,
    /// The recorded condition.
    pub before: Condition,
    /// The desired condition.
    pub after: Condition,
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
        self.conditions.is_empty() && self.timeout.is_none()
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
    before: &Condition,
    after: &Condition,
) -> Option<ConditionChange> {
    if before.simplified() == after.simplified() {
        return None;
    }
    Some(ConditionChange {
        slot,
        before: before.clone(),
        after: after.clone(),
    })
}

/// Build one timeout change while caps differ.
fn timeout_change(before: u64, after: u64) -> Option<TimeoutChange> {
    if before == after {
        return None;
    }
    Some(TimeoutChange { before, after })
}

/// Build one hook modification across conditions and timeout.
///
/// - Conditions collect in slot order.
fn modify_hook(recorded: &Hook, hook: &Hook) -> HookModification {
    let mut conditions = Vec::new();
    if let Some(change) =
        condition_change(ConditionSlot::Requires, &recorded.requires, &hook.requires)
    {
        conditions.push(change);
    }
    if let Some(change) = condition_change(ConditionSlot::When, &recorded.when, &hook.when) {
        conditions.push(change);
    }
    if let Some(change) = condition_change(ConditionSlot::Check, &recorded.check, &hook.check) {
        conditions.push(change);
    }
    HookModification {
        conditions,
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
            timeout_secs,
            id: None,
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
        assert!(lifecycle[0].is_silent());
        assert!(lifecycle[1].is_added());
        assert!(lifecycle[2].is_removed());
    }

    #[test]
    fn lifecycle_reads_simplified_conditions_silent() {
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
        assert!(lifecycle[0].is_silent());
    }

    #[test]
    fn lifecycle_reports_check_slot_change() {
        let before = hook(&["mise"], &[], Condition::default(), in_path("a"), 60);
        let after = hook(&["mise"], &[], Condition::default(), in_path("b"), 60);
        let current = [after];
        let previous = [before];
        let lifecycle = diff_lifecycle(&current, &previous);
        assert_eq!(lifecycle.len(), 1);
        let edits = match &lifecycle[0].change {
            HookChange::Modified(edits) => edits,
            _ => panic!("check diff is modified"),
        };
        assert_eq!(edits.conditions.len(), 1);
        assert_eq!(edits.conditions[0].slot, ConditionSlot::Check);
        assert_eq!(edits.conditions[0].before, in_path("a"));
        assert_eq!(edits.conditions[0].after, in_path("b"));
    }

    #[test]
    fn lifecycle_holds_timeout_edit() {
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
    fn checks_arrays_fail_with_unknown_field() {
        let lua = Lua::new();
        let value: Value = lua
            .load(r#"return { argv = { "mise" }, checks = { { in_path = { name = "x" } } } }"#)
            .call(())
            .unwrap();
        match Hook::decode(&value, &lua) {
            Ok(_) => panic!("checks arrays pass"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("unknown field") && text.contains("checks"),
                    "the text shows the dead key: {text}"
                );
            }
        }
    }

    #[test]
    fn timeout_round_trips_through_strings() {
        use confit_binding::value::Encode;

        let lua = Lua::new();
        let source = hook(
            &["tool"],
            &[],
            Condition::default(),
            Condition::default(),
            90,
        );
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
