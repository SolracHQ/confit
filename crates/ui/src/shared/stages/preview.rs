//! Preview blocks and lines.

use confit_differ::{change::Change, view::View};
use extend::ext;

use crate::shared::{
    components::{
        blocks::{DisplayBlock, Docs},
        labels::{preview_group_text, row_symbol, row_text},
        style::{Marks, Symbol, strExt as _},
        tables::preview_table,
    },
    stages::hooks::HookStage as _,
};

#[ext(name = PreviewStage)]
pub(crate) impl View {
    /// Builds preview blocks.
    fn preview_blocks(&self) -> Vec<DisplayBlock> {
        let documents = Docs::collect(self);
        let rows: Vec<&Change> = self.rows.iter().collect();
        let mut blocks = Vec::new();
        for (root, entries) in Docs::cluster(&rows, &documents) {
            let children = preview_children(self, &entries);
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

    /// Builds the preview.
    fn preview(&self) -> Vec<String> {
        let mut sections: Vec<Vec<String>> = vec![
            vec!["Comparison".title()],
            vec!["Documents".header()],
            DisplayBlock::render(&self.preview_blocks())
                .iter()
                .map(|line| line.paint())
                .collect(),
        ];
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

/// Builds preview children.
fn preview_children(view: &View, documents: &[&Docs<'_>]) -> Vec<DisplayBlock> {
    let mut blocks = Vec::new();
    for document in documents {
        match document {
            Docs::Doc { change } => {
                let label = row_text(view, change, &change.path.display());
                let symbol = row_symbol(change);
                let leaf = DisplayBlock::leaf(Marks::plan(symbol), label);
                blocks.push(leaf);
            }
            Docs::Group { group, members } => {
                let rows = Docs::rows(members);
                let label = preview_group_text(&rows, &group.display());
                let symbol = Symbol::from_members(&rows);
                let leaf = DisplayBlock::leaf(Marks::plan(symbol), label);
                blocks.push(leaf);
            }
            Docs::Dir { .. } => continue,
        }
    }
    blocks
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
    use confit_model::document::{Data, Document, MemberKey, TreeDocument, TreeMember};
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
        }
    }

    /// Builds one empty view.
    fn bare(rows: Vec<Change>) -> View {
        let empty = Arc::new(manifest(Vec::new()));
        View::new(ViewKind::Desired, rows, Arc::clone(&empty), empty)
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
    fn preview_keeps_rows_in_order() {
        let seen = bare(vec![
            single("/tmp/apex/beta", ChangeKind::Addition),
            single("/tmp/apex/alpha", ChangeKind::Removal),
        ]);
        let blocks = seen.preview_blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].mark.symbol, None);
        assert_eq!(blocks[0].mark.outcome, None);
        assert_eq!(blocks[0].children.len(), 2);
        assert_eq!(blocks[0].children[0].text, "/tmp/apex/beta");
        assert_eq!(blocks[0].children[1].text, "/tmp/apex/alpha");
        assert_eq!(blocks[0].children[0].mark.symbol, Some(Symbol::Add));
        assert_eq!(blocks[0].children[1].mark.symbol, Some(Symbol::Remove));
    }

    #[test]
    fn preview_splits_clusters_by_top_key() {
        let seen = bare(vec![
            single("/home/user/alpha", ChangeKind::Addition),
            single("/cache/beta", ChangeKind::Addition),
        ]);
        let blocks = seen.preview_blocks();
        assert_eq!(blocks.len(), 2);
        for branch in &blocks {
            assert_eq!(branch.mark.symbol, None);
            assert_eq!(branch.mark.outcome, None);
            assert_eq!(branch.children.len(), 1);
        }
        assert_eq!(blocks[0].children[0].text, "/cache/beta");
        assert_eq!(blocks[1].children[0].text, "/home/user/alpha");
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
        assert!(leaf.text.contains("tree"));
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
        assert!(leaf.text.contains("tree"));
    }
}
