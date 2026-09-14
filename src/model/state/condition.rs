//! Condition
//!
//! Shell conditions as data.

use serde::{Deserialize, Serialize};

/// Describes a predicate over shell-session facts, held as data.
///
/// Serialized shape uses externally tagged snake_case names, with `Not` serializing as `nop`
/// matching the Lua constructor `confit.shell.nop`: `{ "env_eq": { "key": "..", "value": ".."
/// } }`, `{ "env_set": { "key": ".." } }`, `{ "in_path": { "name": ".." } }`, `{ "exists":
/// { "path": ".." } }`, `{ "all": [...] }`, `{ "any": [...] }`, `{ "nop": ... }`.
///
/// The closed enum covers every shape. Optionality lives in `Option<Condition>` at use sites,
/// with `None` marking unconditional entries. `All`/`Any` compare pairwise in order: `any(a,
/// b)` ranks apart from `any(b, a)` by design; commutativity stays unnormalized, both orderings
/// win, output carries redundancy.
///
/// # Examples
/// ```rust
/// use confit::model::state::condition::Condition;
///
/// let a = Condition::EnvEq { key: "TERM_PROGRAM".into(), value: "WarpTerminal".into() };
/// let b = Condition::EnvEq { key: "TERM_PROGRAM".into(), value: "WarpTerminal".into() };
/// assert!(a == b);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Asserts equality between a shell variable and a value.
    ///
    /// # Arguments
    ///
    /// * `key` - the variable name.
    /// * `value` - the expected value.
    EnvEq { key: String, value: String },
    /// Asserts a shell variable carries a set, non-empty value.
    ///
    /// # Arguments
    ///
    /// * `key` - the variable name.
    EnvSet { key: String },
    /// Asserts a binary resolves on PATH.
    ///
    /// # Arguments
    ///
    /// * `name` - the binary name.
    InPath { name: String },
    /// Asserts a path exists.
    ///
    /// # Arguments
    ///
    /// * `path` - the path under test.
    Exists { path: String },
    /// Combines nested conditions as a conjunction.
    ///
    /// Element order is significant for equality.
    ///
    /// # Arguments
    ///
    /// * conjuncts - in declaration order.
    All(Vec<Condition>),
    /// Combines nested conditions as a disjunction.
    ///
    /// Element order is significant for equality.
    ///
    /// # Arguments
    ///
    /// * disjuncts - in declaration order.
    Any(Vec<Condition>),
    /// Negates a nested condition.
    ///
    /// Serializes as `nop`.
    ///
    /// # Arguments
    ///
    /// * inner condition.
    #[serde(rename = "nop")]
    Not(Box<Condition>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warp() -> Condition {
        Condition::EnvEq {
            key: "TERM_PROGRAM".into(),
            value: "WarpTerminal".into(),
        }
    }

    fn ssh() -> Condition {
        Condition::EnvSet {
            key: "SSH_TTY".into(),
        }
    }

    fn on_path() -> Condition {
        Condition::InPath {
            name: "mybin".into(),
        }
    }

    fn seed_path() -> Condition {
        Condition::Exists {
            path: "/data/seed".into(),
        }
    }

    #[test]
    fn nil_equals_only_nil() {
        let none_a: Option<Condition> = None;
        let none_b: Option<Condition> = None;
        assert!(none_a == none_b);
        assert!(none_a != Some(warp()));
        assert!(Some(warp()) != none_a);
        assert!(Some(warp()) == Some(warp()));
    }

    #[test]
    fn leaf_shapes_compare_by_value() {
        assert!(warp() == warp());
        assert!(
            warp()
                != Condition::EnvEq {
                    key: "TERM_PROGRAM".into(),
                    value: "iTerm".into(),
                }
        );
        assert!(warp() != ssh());
        assert!(ssh() == ssh());
        assert!(ssh() != Condition::EnvSet { key: "TERM".into() });
        assert!(on_path() == on_path());
        assert!(
            on_path()
                != Condition::InPath {
                    name: "otherbin".into(),
                }
        );
        assert!(on_path() != ssh());
        assert!(seed_path() == seed_path());
        assert!(
            seed_path()
                != Condition::Exists {
                    path: "/data/other".into(),
                }
        );
        assert!(seed_path() != on_path());
    }

    #[test]
    fn all_any_are_order_sensitive() {
        let ordered = Condition::All(vec![warp(), ssh()]);
        assert!(ordered == Condition::All(vec![warp(), ssh()]));
        assert!(ordered != Condition::All(vec![ssh(), warp()]));
        assert!(ordered != Condition::Any(vec![warp(), ssh()]));
        assert!(ordered != Condition::All(vec![warp()]));
        assert!(ordered != Condition::All(vec![warp(), ssh(), warp()]));

        let any = Condition::Any(vec![warp(), ssh()]);
        assert!(any != Condition::Any(vec![ssh(), warp()]));
    }

    #[test]
    fn not_nests_recursively() {
        let not_warp = Condition::Not(Box::new(warp()));
        assert!(not_warp == Condition::Not(Box::new(warp())));
        assert!(not_warp != warp());
        assert!(not_warp != Condition::Not(Box::new(ssh())));

        let double = Condition::Not(Box::new(not_warp.clone()));
        assert!(double == Condition::Not(Box::new(not_warp)));

        let not_all = Condition::Not(Box::new(Condition::All(vec![warp(), ssh()])));
        let not_all_swapped = Condition::Not(Box::new(Condition::All(vec![ssh(), warp()])));
        assert!(not_all != not_all_swapped);
    }

    #[test]
    fn serde_shape_is_stable_and_tagged() {
        assert_eq!(
            serde_json::to_value(warp()).unwrap(),
            serde_json::json!({"env_eq": {"key": "TERM_PROGRAM", "value": "WarpTerminal"}})
        );
        assert_eq!(
            serde_json::to_value(ssh()).unwrap(),
            serde_json::json!({"env_set": {"key": "SSH_TTY"}})
        );
        assert_eq!(
            serde_json::to_value(Condition::Not(Box::new(ssh()))).unwrap(),
            serde_json::json!({"nop": {"env_set": {"key": "SSH_TTY"}}})
        );
        assert_eq!(
            serde_json::to_value(on_path()).unwrap(),
            serde_json::json!({"in_path": {"name": "mybin"}})
        );
        assert_eq!(
            serde_json::to_value(seed_path()).unwrap(),
            serde_json::json!({"exists": {"path": "/data/seed"}})
        );
        let all = Condition::All(vec![warp()]);
        let round_tripped: Condition =
            serde_json::from_value(serde_json::to_value(&all).unwrap()).unwrap();
        assert!(all == round_tripped);
        let guarded = Condition::All(vec![on_path(), seed_path()]);
        let round_tripped: Condition =
            serde_json::from_value(serde_json::to_value(&guarded).unwrap()).unwrap();
        assert!(guarded == round_tripped);
    }
}
