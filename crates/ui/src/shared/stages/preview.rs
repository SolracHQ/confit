//! Preview blocks and lines.

use std::collections::BTreeMap;
use std::path::{Component, Path};

use confit_differ::view::View;
use confit_model::routes::RouteBase;
use extend::ext;

use crate::shared::{
    components::{
        blocks::{DisplayBlock, Docs},
        labels::{preview_group_text, row_symbol, row_text},
        style::{Drift, Marks, Symbol, strExt as _},
        tables::preview_table,
    },
    stages::hooks::HookStage as _,
};

#[ext(name = PreviewStage)]
pub(crate) impl View {
    /// Builds preview blocks.
    fn preview_blocks(&self) -> Vec<DisplayBlock> {
        let documents = Docs::collect(self);
        let mut by_base: BTreeMap<RouteBase, Vec<&Docs>> = BTreeMap::new();
        for document in &documents {
            let base = match document {
                Docs::Doc { change } => change.path.base(),
                Docs::Group { group, .. } => group.base(),
                Docs::Dir { .. } => continue,
            };
            by_base.entry(base).or_default().push(document);
        }
        let mut blocks = Vec::new();
        for (base, entries) in &by_base {
            let label = format!("[{}]", base.name());
            let children = if *base == RouteBase::Literal {
                literal_children(self, entries)
            } else {
                nested_children(self, entries)
            };
            blocks.push(DisplayBlock::branch(
                Marks {
                    outcome: None,
                    symbol: None,
                    drift: None,
                },
                label,
                children,
            ));
        }
        blocks
    }

    /// Builds the preview.
    fn preview(&self) -> Vec<String> {
        let mut documents = vec!["Documents".header()];
        let rendered = DisplayBlock::render(&self.preview_blocks());
        for line in &rendered {
            if line.depth == 1 {
                let drift = match line.marks.drift {
                    Some(mark) => format!("{mark} "),
                    None => String::new(),
                };
                documents.push(format!(
                    "{}{drift}{}",
                    line.prefix,
                    line.label.as_str().kind()
                ));
            } else {
                documents.push(line.paint());
            }
        }
        let mut sections = vec![vec!["Comparison".title()], documents];
        let lines = DisplayBlock::render(&self.hooks());
        if !lines.is_empty() {
            let mut hook_section = vec!["Hooks".header()];
            hook_section.extend(lines.iter().map(|line| line.paint()));
            sections.push(hook_section);
        }
        let mut summary = vec!["Summary".title()];
        summary.extend(preview_table(self).lines().map(str::to_owned));
        sections.push(summary);
        let mut out = Vec::new();
        for section in sections
            .into_iter()
            .filter(|section| section.iter().any(|line| !line.is_empty()))
        {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.extend(section);
        }
        out
    }
}

/// Builds literal leaves with full paths.
fn literal_children(view: &View, documents: &[&Docs<'_>]) -> Vec<DisplayBlock> {
    let mut blocks = Vec::new();
    for document in documents {
        match document {
            Docs::Doc { change } => {
                let label = row_text(view, change, &change.path.display());
                let mut marks = Marks::plan(row_symbol(change));
                if change.drifted {
                    marks.drift = Some(Drift);
                }
                blocks.push(DisplayBlock::leaf(marks, label));
            }
            Docs::Group { group, members } => {
                let changes = Docs::changes(members);
                let label = preview_group_text(&changes, &group.display());
                let mut marks = Marks::plan(Symbol::from_members(&changes));
                if changes.iter().any(|change| change.drifted) {
                    marks.drift = Some(Drift);
                }
                blocks.push(DisplayBlock::leaf(marks, label));
            }
            Docs::Dir { .. } => continue,
        }
    }
    blocks
}

/// Builds nested leaves over relative paths.
fn nested_children(view: &View, documents: &[&Docs<'_>]) -> Vec<DisplayBlock> {
    let mut children = Vec::new();
    for document in documents {
        match document {
            Docs::Doc { change } => {
                let parts = relative_parts(change.path.relative_path());
                let (dirs, leaf) = split_parts(&parts, change.path.relative());
                let label = row_text(view, change, &leaf);
                let mut marks = Marks::plan(row_symbol(change));
                if change.drifted {
                    marks.drift = Some(Drift);
                }
                insert_leaf(&mut children, &dirs, DisplayBlock::leaf(marks, label));
            }
            Docs::Group { group, members } => {
                let changes = Docs::changes(members);
                let parts = relative_parts(group.relative_path());
                let (dirs, leaf) = split_parts(&parts, group.relative());
                let label = preview_group_text(&changes, &leaf);
                let mut marks = Marks::plan(Symbol::from_members(&changes));
                if changes.iter().any(|change| change.drifted) {
                    marks.drift = Some(Drift);
                }
                insert_leaf(&mut children, &dirs, DisplayBlock::leaf(marks, label));
            }
            Docs::Dir { .. } => continue,
        }
    }
    children
}

/// Splits directory parts from the leaf name.
fn split_parts(parts: &[String], fallback: String) -> (Vec<String>, String) {
    match parts.split_last() {
        Some((leaf, dirs)) => (dirs.to_vec(), leaf.clone()),
        None => (Vec::new(), fallback),
    }
}

/// Returns normal path components.
fn relative_parts(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|part| match part {
            Component::Normal(found) => Some(found.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect()
}

/// Inserts one leaf under nested directories.
fn insert_leaf(children: &mut Vec<DisplayBlock>, dirs: &[String], leaf: DisplayBlock) {
    let mut current = children;
    for part in dirs {
        current = dir_children_mut(current, part);
    }
    current.push(leaf);
}

/// Returns the child directory.
fn dir_children_mut<'a>(
    children: &'a mut Vec<DisplayBlock>,
    name: &str,
) -> &'a mut Vec<DisplayBlock> {
    let mut position = None;
    for (index, child) in children.iter().enumerate() {
        if child.text == name && child.mark.symbol.is_none() {
            position = Some(index);
            break;
        }
    }
    if position.is_none() {
        children.push(DisplayBlock::branch(
            Marks {
                outcome: None,
                symbol: None,
                drift: None,
            },
            name.to_owned(),
            Vec::new(),
        ));
        position = Some(children.len() - 1);
    }
    let index = match position {
        Some(found) => found,
        None => unreachable!("missing directory"),
    };
    &mut children[index].children
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use confit_core::resource_ref::ResourceRef;
    use confit_core::sha::Sha;
    use confit_differ::change::{Change, ChangeKind, Comparability};
    use confit_differ::diff::Diff;
    use confit_differ::view::{View, ViewKind};
    use confit_model::document::{
        Data, Document, FileMode, MemberKey, TextDocument, TreeDocument, TreeMember,
    };
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};

    use super::PreviewStage as _;
    use crate::shared::components::style::Symbol;

    /// Builds one literal route.
    fn literal(path: &str) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    /// Builds one comparable change with no group.
    fn single(path: &str, kind: ChangeKind) -> Change {
        let route = literal(path);
        Change {
            path: route.clone(),
            expanded: PathBuf::from(path),
            kind,
            comparability: Comparability::Comparable,
            group: None,
            before: None,
            after: None,
            drifted: false,
        }
    }

    /// Builds one based change with a fake host path.
    fn based(base: RouteBase, relative: &str, kind: ChangeKind) -> Change {
        let route = Route::new(base, relative).unwrap();
        Change {
            path: route.clone(),
            expanded: PathBuf::from(format!("/tmp/confit-fake-host/{relative}")),
            kind,
            comparability: Comparability::Comparable,
            group: None,
            before: None,
            after: None,
            drifted: false,
        }
    }

    /// Builds one empty view.
    fn bare(changes: Vec<Change>) -> View {
        let empty = Arc::new(manifest(Vec::new()));
        View::new(ViewKind::Desired, changes, Arc::clone(&empty), empty)
    }

    /// Builds one manifest from documents.
    fn manifest(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents,
            hooks: Vec::new(),
        }
    }

    /// Builds one text document.
    fn text(route: Route, content: &str) -> Document {
        Document::new(
            route,
            false,
            Data::Text(TextDocument {
                content: content.to_owned(),
                mode: FileMode::default(),
            }),
        )
    }

    /// Builds one tree document.
    fn tree(route: Route, members: &[(&str, &[u8])]) -> Document {
        let mut table = BTreeMap::new();
        for (name, bytes) in members {
            let key = MemberKey::new((*name).to_owned()).unwrap();
            let member = TreeMember::Handle(ResourceRef::new(Sha::hash(bytes)));
            table.insert(key, member);
        }
        Document::new(route, false, Data::Tree(TreeDocument { members: table }))
    }

    #[test]
    fn preview_groups_documents_by_base_with_brackets() {
        let folder = tempfile::tempdir().unwrap();
        let apex = folder.path().join("apex").display().to_string();
        let wanted = manifest(vec![
            text(
                Route::new(RouteBase::Home, ".confit-preview-rc").unwrap(),
                "shell",
            ),
            text(
                Route::new(RouteBase::Config, "confit-preview/theme.yml").unwrap(),
                "theme",
            ),
            text(literal(&apex), "apex"),
        ]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let blocks = diff.desired.preview_blocks();
        assert_eq!(blocks.len(), 3);
        for branch in &blocks {
            assert_eq!(branch.mark.symbol, None);
            assert_eq!(branch.mark.outcome, None);
        }
        let home_leaf = &blocks[1].children[0];
        assert_eq!(home_leaf.mark.symbol, Some(Symbol::Add));
        let dir = &blocks[0].children[0];
        assert_eq!(dir.mark.symbol, None);
        assert_eq!(dir.children.len(), 1);
    }

    #[test]
    fn preview_nests_relative_paths_under_dirs() {
        let wanted = manifest(vec![
            text(
                Route::new(RouteBase::Home, "confit-nest/alpha").unwrap(),
                "alpha",
            ),
            text(
                Route::new(RouteBase::Home, "confit-nest/beta").unwrap(),
                "beta",
            ),
        ]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let blocks = diff.desired.preview_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].children.len(), 1);
        let dir = &blocks[0].children[0];
        assert_eq!(dir.mark.symbol, None);
        assert_eq!(dir.children.len(), 2);
    }

    #[test]
    fn preview_lists_literals_flat_with_full_paths() {
        let seen = bare(vec![
            single("/tmp/apex/beta", ChangeKind::Addition),
            single("/tmp/apex/alpha", ChangeKind::Removal),
        ]);
        let blocks = seen.preview_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].children.len(), 2);
        assert_eq!(blocks[0].children[0].mark.symbol, Some(Symbol::Add));
        assert_eq!(blocks[0].children[1].mark.symbol, Some(Symbol::Remove));
        for leaf in &blocks[0].children {
            assert!(leaf.children.is_empty());
        }
    }

    #[test]
    fn preview_keeps_drift_marks_on_leaves() {
        let mut change = based(RouteBase::Home, ".confit-drift-rc", ChangeKind::Addition);
        change.drifted = true;
        let seen = bare(vec![change]);
        let blocks = seen.preview_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].children.len(), 1);
        assert!(blocks[0].children[0].mark.drift.is_some());
    }

    #[test]
    fn preview_collapses_group_to_one_leaf() {
        let folder = tempfile::tempdir().unwrap();
        let group = Route::new(RouteBase::Literal, folder.path().join("shared")).unwrap();
        let wanted = manifest(vec![tree(
            group,
            &[("one.txt", b"one"), ("two.txt", b"two")],
        )]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let blocks = diff.desired.preview_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].children.len(), 1);
        let leaf = &blocks[0].children[0];
        assert_eq!(leaf.mark.symbol, Some(Symbol::Add));
        assert_eq!(leaf.mark.outcome, None);
        assert_eq!(leaf.children.len(), 0);
    }

    #[test]
    fn preview_marks_mixed_group_as_update() {
        let folder = tempfile::tempdir().unwrap();
        let group = Route::new(RouteBase::Literal, folder.path().join("shared")).unwrap();
        let wanted = manifest(vec![tree(group.clone(), &[("one.txt", b"one")])]);
        let recorded = manifest(vec![tree(group, &[("two.txt", b"two")])]);
        let diff = Diff::build(wanted, recorded).unwrap();
        let blocks = diff.desired.preview_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].children.len(), 1);
        let leaf = &blocks[0].children[0];
        assert_eq!(leaf.mark.symbol, Some(Symbol::Update));
        assert_eq!(leaf.children.len(), 0);
    }
}
