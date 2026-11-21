//! Outcome blocks and lines.

use std::path::{Path, PathBuf};

use confit_differ::{change::Change, view::View};
use confit_events::protocol::ApplyOutcome;
use confit_model::{
    document::{Data, Document},
    routes::Route,
};
use extend::ext;

use crate::shared::{
    components::{
        blocks::{DisplayBlock, Docs},
        labels::{KIND_TREE, row_symbol, row_text},
        style::{Marks, Symbol},
        tables::{DocCounts, clause},
    },
    state::OutcomeMap,
};

#[ext(name = OutcomeStage)]
pub(crate) impl View {
    /// Builds outcome blocks.
    fn outcome_blocks(&self, outcomes: &OutcomeMap) -> Vec<DisplayBlock> {
        let documents = Docs::collect(self);
        let rows: Vec<&Change> = self.rows.iter().collect();
        let mut blocks = Vec::new();
        for (root, entries) in Docs::cluster(&rows, &documents) {
            let factored = Docs::factor(&root, &entries);
            let children = match factored {
                Docs::Dir { children, .. } => outcome_children(self, &children, outcomes),
                Docs::Doc { .. } | Docs::Group { .. } => Vec::new(),
            };
            let branch = DisplayBlock::branch(
                Marks {
                    outcome: None,
                    symbol: None,
                },
                root.display().to_string(),
                children,
            );
            blocks.push(branch);
        }
        blocks
    }

    /// Builds the outcome.
    fn outcome(&self, outcomes: &OutcomeMap) -> Vec<String> {
        if outcomes.is_empty() {
            return Vec::new();
        }
        DisplayBlock::render(&self.outcome_blocks(outcomes))
            .iter()
            .map(|line| line.paint())
            .collect()
    }
}

/// Returns the landed member count.
fn group_landed(members: &[&Change], outcomes: &OutcomeMap) -> usize {
    members
        .iter()
        .filter(|found| {
            matches!(
                outcomes.get(&found.expanded),
                Some(ApplyOutcome::Added | ApplyOutcome::Changed | ApplyOutcome::Removed)
            )
        })
        .count()
}

/// Returns the total member count.
fn group_total(view: &View, group: &Route, members: &[&Change]) -> usize {
    match view.find_document(group) {
        Some(Document {
            data: Data::Tree(tree),
            ..
        }) => tree.members.len(),
        _ => members.len(),
    }
}

/// Returns the landed and total counts.
fn group_progress(
    view: &View,
    group: &Route,
    members: &[&Change],
    outcomes: &OutcomeMap,
) -> (usize, usize) {
    (
        group_landed(members, outcomes),
        group_total(view, group, members),
    )
}

/// Builds outcome children.
fn outcome_children(
    view: &View,
    documents: &[Docs<'_>],
    outcomes: &OutcomeMap,
) -> Vec<DisplayBlock> {
    let mut branches: Vec<(String, &Docs<'_>)> = Vec::new();
    let mut files: Vec<(String, &Change)> = Vec::new();
    for document in documents {
        match document {
            Docs::Dir { name, .. } => {
                branches.push((name.clone(), document));
            }
            Docs::Group { group, .. } => {
                let label = leaf_name(&group_host_path(group));
                branches.push((label, document));
            }
            Docs::Doc { change } => {
                let label = leaf_name(&change.expanded);
                files.push((label, *change));
            }
        }
    }
    branches.sort_by(|left, right| left.0.cmp(&right.0));
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut blocks = Vec::new();
    for (name, document) in branches {
        match document {
            Docs::Dir { children, .. } => {
                let inner = outcome_children(view, children, outcomes);
                let branch = DisplayBlock::branch(
                    Marks {
                        outcome: None,
                        symbol: None,
                    },
                    name,
                    inner,
                );
                blocks.push(branch);
            }
            Docs::Group { group, members } => {
                let rows = Docs::rows(members);
                let label = outcome_group_text(view, group, &rows, &name, outcomes);
                let symbol = Symbol::from_members(&rows);
                let mut branch = DisplayBlock::leaf(Marks::plan(symbol), label);
                branch.apply(outcomes.group_outcome(&rows));
                branch.children = outcome_children(view, members, outcomes);
                blocks.push(branch);
            }
            Docs::Doc { .. } => continue,
        }
    }
    for (name, change) in files {
        let label = row_text(view, change, &name);
        let symbol = row_symbol(change);
        let mut leaf = DisplayBlock::leaf(Marks::plan(symbol), label);
        leaf.apply(outcomes.outcome_for(change));
        blocks.push(leaf);
    }
    blocks
}

/// Returns the outcome group text.
fn outcome_group_text(
    view: &View,
    group: &Route,
    members: &[&Change],
    name: &str,
    outcomes: &OutcomeMap,
) -> String {
    format!(
        "{name}: {KIND_TREE} {}",
        outcome_clause(view, group, members, outcomes)
    )
}

/// Returns the file name with fallback to the display.
fn leaf_name(path: &Path) -> String {
    path.file_name()
        .map(|found| found.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Returns the host path.
fn group_host_path(group: &Route) -> PathBuf {
    match group.resolve() {
        Some(found) => found,
        None => PathBuf::from(group.display()),
    }
}

/// Returns the outcome clause.
fn outcome_clause(
    view: &View,
    group: &Route,
    members: &[&Change],
    outcomes: &OutcomeMap,
) -> String {
    let (added, changed, removed, failed) = ApplyOutcome::count(
        members
            .iter()
            .filter_map(|found| outcomes.get(&found.expanded)),
    );
    let docs = DocCounts {
        added,
        changed,
        removed,
        failed,
    };
    let (landed, total) = group_progress(view, group, members, outcomes);
    debug_assert_eq!(landed, docs.added + docs.changed + docs.removed);
    let unattempted = total.saturating_sub(landed + docs.failed);
    clause(&docs, unattempted)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use confit_core::resource_ref::ResourceRef;
    use confit_core::sha::Sha;
    use confit_differ::diff::Diff;
    use confit_events::protocol::ApplyOutcome;
    use confit_model::document::{
        Data, Document, FileMode, MemberKey, TextDocument, TreeDocument, TreeMember,
    };
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};

    use super::OutcomeStage as _;
    use crate::shared::components::style::{Outcome, Symbol};
    use crate::shared::state::OutcomeMap;

    /// Builds one literal route.
    fn literal(path: &str) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    /// Builds one text document with default mode.
    fn text(route: Route, content: &str) -> Document {
        Document::new(
            route,
            Data::Text(TextDocument {
                content: content.to_owned(),
                mode: FileMode::default(),
                unmanaged: false,
            }),
        )
    }

    /// Builds one manifest from documents.
    fn manifest(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents,
            hooks: Vec::new(),
        }
    }

    /// Builds one tree document.
    fn tree(route: Route, members: &[(&str, &[u8])]) -> Document {
        let mut table = BTreeMap::new();
        for (name, bytes) in members {
            let key = MemberKey::new((*name).to_owned()).unwrap();
            let member = TreeMember::Handle(ResourceRef::new(Sha::hash(bytes)));
            table.insert(key, member);
        }
        Document::new(route, Data::Tree(TreeDocument { members: table }))
    }

    #[test]
    fn outcome_nests_config_with_dirs_first() {
        let folder = tempfile::tempdir().unwrap();
        let base = folder.path().to_string_lossy().into_owned();
        let first = format!("{base}/.bashrc");
        let second = format!("{base}/.config/alpha");
        let third = format!("{base}/.config/beta");
        let wanted = manifest(vec![
            text(literal(&first), "shell"),
            text(literal(&second), "alpha"),
            text(literal(&third), "beta"),
        ]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let seen = &diff.effective;
        let mut outcomes = OutcomeMap::new();
        for change in &seen.rows {
            outcomes.record(change.expanded.clone(), ApplyOutcome::Added);
        }
        let blocks = seen.outcome_blocks(&outcomes);
        assert_eq!(blocks.len(), 1);
        let root = &blocks[0];
        assert_eq!(root.mark.symbol, None);
        assert_eq!(root.mark.outcome, None);
        assert_eq!(root.children.len(), 2);
        assert!(!root.children[0].children.is_empty());
        assert!(root.children[1].children.is_empty());
        assert!(root.children[0].text.contains(".config"));
        assert!(root.children[1].text.contains(".bashrc"));
        let nested = &root.children[0];
        assert_eq!(nested.children.len(), 2);
        assert!(nested.children[0].text.contains("alpha"));
        assert!(nested.children[1].text.contains("beta"));
        for leaf in [&root.children[1], &nested.children[0]] {
            assert_eq!(leaf.mark.outcome, Some(Outcome::Applied));
            assert_eq!(leaf.mark.symbol, Some(Symbol::Add));
        }
    }

    #[test]
    fn outcome_marks_failed_group_and_unattempted_row() {
        let folder = tempfile::tempdir().unwrap();
        let group = Route::new(RouteBase::Literal, folder.path().join("shared")).unwrap();
        let lone = folder.path().join("lone");
        let wanted = manifest(vec![
            tree(group, &[("one.txt", b"one"), ("two.txt", b"two")]),
            text(literal(&lone.display().to_string()), "lone"),
        ]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let seen = &diff.effective;
        let mut outcomes = OutcomeMap::new();
        for change in &seen.rows {
            if change.expanded.ends_with("one.txt") {
                outcomes.record(change.expanded.clone(), ApplyOutcome::Added);
            }
            if change.expanded.ends_with("two.txt") {
                outcomes.record(change.expanded.clone(), ApplyOutcome::Failed);
            }
        }
        let blocks = seen.outcome_blocks(&outcomes);
        assert_eq!(blocks.len(), 1);
        let mut grouped = None;
        let mut plain = None;
        for child in &blocks[0].children {
            if child.children.is_empty() {
                plain = Some(child);
            } else {
                grouped = Some(child);
            }
        }
        let grouped = grouped.unwrap();
        let plain = plain.unwrap();
        assert_eq!(grouped.mark.outcome, Some(Outcome::Failed));
        assert_eq!(grouped.mark.symbol, Some(Symbol::Add));
        assert_eq!(grouped.children.len(), 2);
        assert_eq!(plain.mark.outcome, Some(Outcome::Unattempted));
    }
}
