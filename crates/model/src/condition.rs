//! Condition
//! Shell session predicates held as data.

use confit_binding_macros::Face;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::routes::Route;

/// Shell session predicate.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Face, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
#[face(table, tagged)]
pub enum Condition {
    /// Equality between a shell variable and a value.
    EnvEq {
        /// The variable name.
        key: String,
        /// The expected value.
        value: String,
    },
    /// Non-empty variable assertion.
    EnvSet {
        /// The variable name.
        key: String,
    },
    /// A binary on PATH assertion.
    InPath {
        /// The binary name.
        name: String,
    },
    /// A path existence assertion.
    Exists {
        /// The route under testing.
        route: Route,
    },
    /// A changed document assertion.
    Changed {
        /// The document route under testing.
        route: Route,
    },
    /// A conjunction of nested conditions.
    All(Vec<Condition>),
    /// A disjunction of nested conditions.
    Any(Vec<Condition>),
    /// A negated nested condition.
    #[serde(rename = "nop")]
    #[face(tag = "nop")]
    Not(Box<Condition>),
}

/// N-ary operator for folding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NaryKind {
    /// `All` member list.
    All,
    /// `Any` member list.
    Any,
}

impl Condition {
    /// Report whether one condition tree holds a changed leaf.
    pub fn holds_changed(&self) -> bool {
        match self {
            Self::Changed { .. } => true,
            Self::All(items) | Self::Any(items) => items.iter().any(Self::holds_changed),
            Self::Not(inner) => inner.holds_changed(),
            Self::EnvEq { .. }
            | Self::EnvSet { .. }
            | Self::InPath { .. }
            | Self::Exists { .. } => false,
        }
    }

    /// Collect changed destinations in tree order.
    pub fn collect_changed(&self, out: &mut Vec<Route>) {
        match self {
            Self::Changed { route: dest } => out.push(dest.clone()),
            Self::All(items) | Self::Any(items) => {
                for item in items {
                    item.collect_changed(out);
                }
            }
            Self::Not(inner) => inner.collect_changed(out),
            Self::EnvEq { .. }
            | Self::EnvSet { .. }
            | Self::InPath { .. }
            | Self::Exists { .. } => {}
        }
    }

    /// Return the simplified form of one condition tree.
    ///
    /// The form is an equivalent tree in idempotent normal form.
    ///
    /// - Empty `All` is true and empty `Any` is false.
    pub fn simplified(&self) -> Self {
        let mut out = self.simplify_step();
        loop {
            let next = out.simplify_step();
            if next == out {
                return out;
            }
            out = next;
        }
    }

    /// Apply one simplification pass over already simple children.
    fn simplify_step(&self) -> Self {
        match self {
            Self::Not(inner) => match inner.simplified() {
                Self::Not(grand) => *grand,
                simple => Self::Not(Box::new(simple)),
            },
            Self::All(items) => Self::nary(NaryKind::All, items),
            Self::Any(items) => Self::nary(NaryKind::Any, items),
            _ => self.clone(),
        }
    }

    /// Simplify one `All` or `Any` member list.
    ///
    /// - Singletons collapse to their member.
    fn nary(kind: NaryKind, items: &[Self]) -> Self {
        let mut simple: Vec<Self> = items.iter().map(Self::simplified).collect();
        let mut flat = Vec::with_capacity(simple.len());
        for item in simple.drain(..) {
            match item {
                Self::All(inner) if kind == NaryKind::All => flat.extend(inner),
                Self::Any(inner) if kind == NaryKind::Any => flat.extend(inner),
                _ => flat.push(item),
            }
        }
        flat.sort();
        flat.dedup();
        if flat.len() == 1 {
            return flat.swap_remove(0);
        }
        if flat.iter().any(|member| Self::cancels(&flat, member)) {
            return Self::nary_empty(kind);
        }
        let absorbed: Vec<bool> = flat
            .iter()
            .map(|member| Self::absorbed(&flat, member, kind))
            .collect();
        let mut kept = Vec::with_capacity(flat.len());
        for (member, drop) in flat.drain(..).zip(absorbed) {
            if !drop {
                kept.push(member);
            }
        }
        flat = kept;
        if flat.len() == 1 {
            return flat.swap_remove(0);
        }
        match kind {
            NaryKind::All => Self::All(flat),
            NaryKind::Any => Self::Any(flat),
        }
    }

    /// Build the empty `All` (true) or `Any` (false) constant.
    fn nary_empty(kind: NaryKind) -> Self {
        match kind {
            NaryKind::All => Self::All(Vec::new()),
            NaryKind::Any => Self::Any(Vec::new()),
        }
    }

    /// Report whether one member meets its complement in the list.
    fn cancels(flat: &[Self], member: &Self) -> bool {
        let negated = Self::Not(Box::new(member.clone()));
        flat.contains(&negated)
    }

    /// Report whether one branch absorbs into a sibling member.
    ///
    /// `All` drops an `Any` branch with any sibling member, and `Any`
    /// drops an `All` branch with any sibling member.
    fn absorbed(flat: &[Self], member: &Self, kind: NaryKind) -> bool {
        let inner = match (kind, member) {
            (NaryKind::All, Self::Any(inner)) | (NaryKind::Any, Self::All(inner)) => inner,
            _ => return false,
        };
        flat.iter().any(|sibling| inner.contains(sibling))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(name: &str) -> Condition {
        Condition::InPath {
            name: name.to_string(),
        }
    }

    #[test]
    fn simplified_flattens_nested_same_ops() {
        let nested = Condition::All(vec![Condition::All(vec![leaf("a"), leaf("b")]), leaf("c")]);
        assert_eq!(
            nested.simplified(),
            Condition::All(vec![leaf("a"), leaf("b"), leaf("c")])
        );
    }

    #[test]
    fn simplified_dedupes_duplicate_members() {
        let dup = Condition::All(vec![leaf("mise"), leaf("mise")]);
        assert_eq!(dup.simplified(), leaf("mise"));
    }

    #[test]
    fn simplified_folds_double_negation() {
        let twice = Condition::Not(Box::new(Condition::Not(Box::new(leaf("a")))));
        assert_eq!(twice.simplified(), leaf("a"));
    }

    #[test]
    fn simplified_absorbs_redundant_branches() {
        let conj = Condition::All(vec![leaf("x"), Condition::Any(vec![leaf("x"), leaf("y")])]);
        assert_eq!(conj.simplified(), leaf("x"));
        let disj = Condition::Any(vec![leaf("x"), Condition::All(vec![leaf("x"), leaf("y")])]);
        assert_eq!(disj.simplified(), leaf("x"));
    }

    #[test]
    fn simplified_folds_complementary_pairs() {
        let conj = Condition::All(vec![leaf("x"), Condition::Not(Box::new(leaf("x")))]);
        assert_eq!(conj.simplified(), Condition::All(Vec::new()));
        let disj = Condition::Any(vec![leaf("x"), Condition::Not(Box::new(leaf("x")))]);
        assert_eq!(disj.simplified(), Condition::Any(Vec::new()));
    }

    #[test]
    fn simplified_sorts_members_for_stable_output() {
        let swapped = Condition::All(vec![leaf("b"), leaf("a")]);
        assert_eq!(
            swapped.simplified(),
            Condition::All(vec![leaf("a"), leaf("b")])
        );
    }
}
