//! Condition
//!
//! Shell session predicates held as data.

use serde::{Deserialize, Serialize};

/// Shell session predicate held as data.
///
/// The plan emits entries unconditionally. Each fresh shell session
/// evaluates the guard and skips entries lacking their binary or state.
///
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Holds equality between a shell variable and a value.
    EnvEq {
        /// Holds the variable name.
        key: String,
        /// Holds the expected value.
        value: String,
    },
    /// Holds a set, non-empty variable assertion.
    EnvSet {
        /// Holds the variable name.
        key: String,
    },
    /// Holds a binary on PATH assertion.
    InPath {
        /// Holds the binary name.
        name: String,
    },
    /// Holds a path existence assertion.
    Exists {
        /// Holds the path under test.
        path: String,
    },
    /// Holds a changed document assertion.
    ///
    /// True while `path` previews a write, drift, or absence.
    /// Hooks only.
    Changed {
        /// Holds the document path under testing.
        path: String,
    },
    /// Holds a conjunction of nested conditions.
    ///
    /// Element order feeds equality.
    All(Vec<Condition>),
    /// Holds a disjunction of nested conditions.
    ///
    /// Element order feeds equality.
    Any(Vec<Condition>),
    /// Holds a negated nested condition.
    ///
    /// Serializes as `nop`.
    #[serde(rename = "nop")]
    Not(Box<Condition>),
}

impl Condition {
    /// Reports whether one condition tree holds a changed leaf.
    ///
    /// # Returns
    ///
    /// True while any nested leaf reads `Changed`.
    ///
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

    /// Collects changed destinations in tree order.
    ///
    /// # Arguments
    ///
    /// * `out` - the destination list gaining one entry per leaf.
    ///
    pub fn collect_changed(&self, out: &mut Vec<String>) {
        match self {
            Self::Changed { path: dest } => out.push(dest.clone()),
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

    /// Returns the simplified form of one condition tree.
    ///
    /// The output evaluates like the input with no redundant
    /// shape left. Empty `All` reads true, empty `Any` reads
    /// false, matching evaluation.
    ///
    /// # Returns
    ///
    /// The equivalent tree in idempotent normal form.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::condition::Condition;
    ///
    /// let dup = Condition::All(vec![
    ///     Condition::InPath { name: "git".into() },
    ///     Condition::InPath { name: "mise".into() },
    ///     Condition::InPath { name: "git".into() },
    /// ]);
    /// assert_eq!(
    ///     dup.simplified(),
    ///     Condition::All(vec![
    ///         Condition::InPath { name: "git".into() },
    ///         Condition::InPath { name: "mise".into() },
    ///     ])
    /// );
    /// ```
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

    /// Applies one simplification pass over already simple children.
    ///
    /// # Returns
    ///
    /// The tree after one round of flattening and folding.
    fn simplify_step(&self) -> Self {
        match self {
            Self::Not(inner) => match inner.simplified() {
                Self::Not(grand) => *grand,
                simple => Self::Not(Box::new(simple)),
            },
            Self::All(items) => Self::nary(true, items),
            Self::Any(items) => Self::nary(false, items),
            _ => self.clone(),
        }
    }

    /// Simplifies one `All` or `Any` member list.
    ///
    /// # Arguments
    ///
    /// * `conjunction` - true for `All`, false for `Any`.
    /// * `items` - the member list under folding.
    ///
    /// # Returns
    ///
    /// The folded tree, collapsing singletons to their member.
    fn nary(conjunction: bool, items: &[Self]) -> Self {
        let mut simple: Vec<Self> = items.iter().map(Self::simplified).collect();
        let mut flat = Vec::with_capacity(simple.len());
        for item in simple.drain(..) {
            match item {
                Self::All(inner) if conjunction => flat.extend(inner),
                Self::Any(inner) if !conjunction => flat.extend(inner),
                _ => flat.push(item),
            }
        }
        flat.sort();
        flat.dedup();
        if flat.len() == 1 {
            return flat.swap_remove(0);
        }
        if flat.iter().any(|member| Self::cancels(&flat, member)) {
            return Self::nary_empty(conjunction);
        }
        let absorbed: Vec<bool> = flat
            .iter()
            .map(|member| Self::absorbed(&flat, member, conjunction))
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
        if conjunction {
            Self::All(flat)
        } else {
            Self::Any(flat)
        }
    }

    /// Builds the empty `All` (true) or `Any` (false) constant.
    ///
    /// # Arguments
    ///
    /// * `conjunction` - true for `All`, false for `Any`.
    ///
    /// # Returns
    ///
    /// The empty member list holding the constant value.
    fn nary_empty(conjunction: bool) -> Self {
        if conjunction {
            Self::All(Vec::new())
        } else {
            Self::Any(Vec::new())
        }
    }

    /// Reports whether one member meets its complement in the list.
    ///
    /// # Arguments
    ///
    /// * `flat` - the flattened member list under checking.
    /// * `member` - the member under testing.
    ///
    /// # Returns
    ///
    /// True while the negated member reads present too.
    fn cancels(flat: &[Self], member: &Self) -> bool {
        let negated = Self::Not(Box::new(member.clone()));
        flat.contains(&negated)
    }

    /// Reports whether one branch absorbs into a sibling member.
    ///
    /// `All` drops an `Any` branch holding any sibling member, and
    /// `Any` drops an `All` branch holding any sibling member.
    ///
    /// # Arguments
    ///
    /// * `flat` - the flattened member list under checking.
    /// * `member` - the branch under testing.
    /// * `conjunction` - true for `All`, false for `Any`.
    ///
    /// # Returns
    ///
    /// True while a sibling member reads inside the branch.
    fn absorbed(flat: &[Self], member: &Self, conjunction: bool) -> bool {
        let inner = match (conjunction, member) {
            (true, Self::Any(inner)) | (false, Self::All(inner)) => inner,
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
