//! Shell session predicates held as data.

use lua_binding::Shape;
use serde::{Deserialize, Serialize};

use crate::routes::Route;

/// A condition over the shell session.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(table, typed_alias)]
pub enum Condition {
    /// A condition that matches a shell variable against a value.
    EnvEq {
        /// The variable name.
        key: String,
        /// The expected value.
        value: String,
    },
    /// A condition that requires a shell variable to hold a value.
    EnvSet {
        /// The variable name.
        key: String,
    },
    /// A condition that requires a binary on PATH.
    InPath {
        /// The binary name.
        name: String,
    },
    /// A condition that requires an existing path.
    Exists {
        /// The path to check.
        route: Route,
    },
    /// A condition that targets a route with pending changes.
    Changed {
        /// The route to check for changes.
        route: Route,
    },
    /// A conjunction of nested conditions.
    All(Vec<Condition>),
    /// A disjunction of nested conditions.
    Any(Vec<Condition>),
    /// A negated nested condition.
    #[serde(rename = "nop")]
    #[shape(variant = "nop")]
    Not(Box<Condition>),
}

/// The operator of an n-ary condition list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NaryKind {
    /// The operator that joins members in a conjunction.
    All,
    /// The operator that joins members in a disjunction.
    Any,
}

impl Default for Condition {
    /// Returns the empty `All` list that always holds.
    fn default() -> Self {
        Self::All(Vec::new())
    }
}

impl Condition {
    /// Returns whether simplification leaves the true constant.
    pub fn is_true(&self) -> bool {
        self.simplified() == Self::default()
    }

    /// Returns whether any leaf targets a changed route.
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

    /// Appends every changed route to the list in tree order.
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

    /// Returns the tree after folding negations and flattening nested lists.
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

    /// Folds one layer of negations and nested lists over simple children.
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

    /// Flattens, sorts, and dedupes one member list.
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

    /// Builds the constant that a cancelled list folds to.
    fn nary_empty(kind: NaryKind) -> Self {
        match kind {
            NaryKind::All => Self::All(Vec::new()),
            NaryKind::Any => Self::Any(Vec::new()),
        }
    }

    /// Returns whether a member and its negation both sit in the list.
    fn cancels(flat: &[Self], member: &Self) -> bool {
        let negated = Self::Not(Box::new(member.clone()));
        flat.contains(&negated)
    }

    /// Returns whether the branch holds a sibling member.
    fn absorbed(flat: &[Self], member: &Self, kind: NaryKind) -> bool {
        let inner = match (kind, member) {
            (NaryKind::All, Self::Any(inner)) | (NaryKind::Any, Self::All(inner)) => inner,
            _ => return false,
        };
        flat.iter().any(|sibling| inner.contains(sibling))
    }

    /// Renders the simplified condition as call text.
    pub fn describe(&self) -> String {
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
        render(&self.simplified())
    }

    /// Returns the removed and added leaves between two conditions.
    pub fn leaf_diff(&self, other: &Self) -> (Vec<Self>, Vec<Self>) {
        let before = Self::flatten(&self.simplified());
        let after = Self::flatten(&other.simplified());
        let removed = before
            .iter()
            .filter(|leaf| !after.contains(leaf))
            .cloned()
            .collect();
        let added = after
            .iter()
            .filter(|leaf| !before.contains(leaf))
            .cloned()
            .collect();
        (removed, added)
    }

    /// Flattens one simplified condition into its member leaves.
    fn flatten(condition: &Self) -> Vec<Self> {
        match condition {
            Self::All(items) | Self::Any(items) => items.iter().flat_map(Self::flatten).collect(),
            _ => vec![condition.clone()],
        }
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

    #[test]
    fn leaf_diff_returns_removed_and_added_leaves() {
        let before = Condition::All(vec![leaf("bat"), leaf("starship")]);
        let after = Condition::All(vec![leaf("bat"), leaf("eza")]);
        assert_eq!(
            before.leaf_diff(&after),
            (vec![leaf("starship")], vec![leaf("eza")])
        );
    }

    #[test]
    fn leaf_diff_flattens_nested_lists_before_compare() {
        let before = Condition::Any(vec![leaf("a")]);
        let after = Condition::All(vec![leaf("a"), Condition::Any(vec![leaf("b"), leaf("c")])]);
        assert_eq!(
            before.leaf_diff(&after),
            (Vec::new(), vec![leaf("b"), leaf("c")])
        );
    }

    #[test]
    fn leaf_diff_returns_empty_lists_for_matching_leaves() {
        let before = Condition::All(vec![leaf("a"), leaf("b")]);
        let after = Condition::Any(vec![leaf("b"), leaf("a")]);
        assert_eq!(before.leaf_diff(&after), (Vec::new(), Vec::new()));
    }
}
