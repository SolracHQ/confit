//! Defines the shell condition ADT plus structural equality.
//!
//! Describes shell-session facts (`TERM_PROGRAM`, `SSH_TTY`, ...) as data.
//! Holds evaluation for later stages; `plan` uses structural equality,
//! driving env/profile shadowing (see `crate::merge`).

use serde::{Deserialize, Serialize};

/// Describes a predicate over shell-session facts, held as data.
///
/// Invariants: the closed enum covers every shape. Optionality lives in
/// `Option<Condition>` at use sites, with `None` marking unconditional
/// entries. `All`/`Any` compare pairwise in order: `any(a, b)` ranks apart
/// from `any(b, a)` by design; commutativity stays unnormalized, both
/// orderings win, output carries redundancy.
///
/// Serialized shape uses externally tagged snake_case names, with `Not`
/// serializing as `not_` matching the Lua constructor `confit.shell.not_`:
/// `{ "env_eq": { "key": "..", "value": ".." } }`,
/// `{ "env_set": { "key": ".." } }`,
/// `{ "all": [...] }`, `{ "any": [...] }`, `{ "not_": ... }`.
///
/// Example:
/// ```rust
/// use confit::model::Condition;
///
/// let a = Condition::EnvEq { key: "TERM_PROGRAM".into(), value: "WarpTerminal".into() };
/// let b = Condition::EnvEq { key: "TERM_PROGRAM".into(), value: "WarpTerminal".into() };
/// assert!(a.structural_eq(&b));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Asserts equality between a shell variable and a value.
    ///
    /// Args: `key` is the variable name, `value` the expected value.
    EnvEq { key: String, value: String },
    /// Asserts a shell variable carries a set, non-empty value.
    ///
    /// Args: `key` is the variable name.
    EnvSet { key: String },
    /// Combines nested conditions as a conjunction.
    ///
    /// Invariants: element order is significant for equality.
    /// Args: the conjuncts, in declaration order.
    All(Vec<Condition>),
    /// Combines nested conditions as a disjunction.
    ///
    /// Invariants: element order is significant for equality.
    /// Args: the disjuncts, in declaration order.
    Any(Vec<Condition>),
    /// Negates a nested condition.
    ///
    /// Serializes as `not_`. Args: the inner condition.
    #[serde(rename = "not_")]
    Not(Box<Condition>),
}

impl Condition {
    /// Compares conditions by structure, leaving evaluation to later stages.
    ///
    /// `EnvEq` matches under agreeing key and value; `EnvSet` under
    /// agreeing keys; `All`/`Any` under matching operator, matching
    /// lengths, plus pairwise equal elements in order; `Not` under equal
    /// inners. Remaining combinations compare unequal.
    /// Args: `other` is the condition to compare against.
    pub fn structural_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::EnvEq {
                    key: self_key,
                    value: self_value,
                },
                Self::EnvEq {
                    key: other_key,
                    value: other_value,
                },
            ) => self_key == other_key && self_value == other_value,
            (Self::EnvSet { key: self_key }, Self::EnvSet { key: other_key }) => {
                self_key == other_key
            }
            (Self::All(self_items), Self::All(other_items))
            | (Self::Any(self_items), Self::Any(other_items)) => {
                self_items.len() == other_items.len()
                    && self_items
                        .iter()
                        .zip(other_items.iter())
                        .all(|(left, right)| left.structural_eq(right))
            }
            (Self::Not(self_inner), Self::Not(other_inner)) => {
                self_inner.structural_eq(other_inner)
            }
            _ => false,
        }
    }
}

/// Compares optional `when` guards at env/profile use sites.
///
/// `None` equals `None`; unconditional and conditional entries occupy
/// separate slots. Paired present conditions compare with
/// [`Condition::structural_eq`].
/// Args: the two guards to compare.
pub fn when_eq(first: Option<&Condition>, second: Option<&Condition>) -> bool {
    match (first, second) {
        (None, None) => true,
        (Some(left), Some(right)) => left.structural_eq(right),
        _ => false,
    }
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

    #[test]
    fn nil_equals_only_nil() {
        assert!(when_eq(None, None));
        assert!(!when_eq(None, Some(&warp())));
        assert!(!when_eq(Some(&warp()), None));
        assert!(when_eq(Some(&warp()), Some(&warp())));
    }

    #[test]
    fn leaf_shapes_compare_by_value() {
        assert!(warp().structural_eq(&warp()));
        assert!(!warp().structural_eq(&Condition::EnvEq {
            key: "TERM_PROGRAM".into(),
            value: "iTerm".into(),
        }));
        assert!(!warp().structural_eq(&ssh()));
        assert!(ssh().structural_eq(&ssh()));
        assert!(!ssh().structural_eq(&Condition::EnvSet { key: "TERM".into() }));
    }

    #[test]
    fn all_any_are_order_sensitive() {
        let ordered = Condition::All(vec![warp(), ssh()]);
        assert!(ordered.structural_eq(&Condition::All(vec![warp(), ssh()])));
        assert!(!ordered.structural_eq(&Condition::All(vec![ssh(), warp()])));
        assert!(!ordered.structural_eq(&Condition::Any(vec![warp(), ssh()])));
        assert!(!ordered.structural_eq(&Condition::All(vec![warp()])));
        assert!(!ordered.structural_eq(&Condition::All(vec![warp(), ssh(), warp()])));

        let any = Condition::Any(vec![warp(), ssh()]);
        assert!(!any.structural_eq(&Condition::Any(vec![ssh(), warp()])));
    }

    #[test]
    fn not_nests_recursively() {
        let not_warp = Condition::Not(Box::new(warp()));
        assert!(not_warp.structural_eq(&Condition::Not(Box::new(warp()))));
        assert!(!not_warp.structural_eq(&warp()));
        assert!(!not_warp.structural_eq(&Condition::Not(Box::new(ssh()))));

        let double = Condition::Not(Box::new(not_warp.clone()));
        assert!(double.structural_eq(&Condition::Not(Box::new(not_warp))));

        let not_all = Condition::Not(Box::new(Condition::All(vec![warp(), ssh()])));
        let not_all_swapped = Condition::Not(Box::new(Condition::All(vec![ssh(), warp()])));
        assert!(!not_all.structural_eq(&not_all_swapped));
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
            serde_json::json!({"not_": {"env_set": {"key": "SSH_TTY"}}})
        );
        let all = Condition::All(vec![warp()]);
        let round_tripped: Condition =
            serde_json::from_value(serde_json::to_value(&all).unwrap()).unwrap();
        assert!(all.structural_eq(&round_tripped));
    }
}
