//! Shows detail options and content.

use std::fmt::{Display, Formatter, Result};

use confit_core::sha::Sha;
use confit_differ::{
    change::Change,
    content::{ContentDiff, KeyDiff, LineDiff, LineTag},
    view::View,
};
use confit_model::{
    document::{Data, FileMode},
    routes::Route,
};

use crate::shared::components::{
    blocks::{DisplayBlock, Docs, Line},
    labels::{preview_group_text, row_symbol, row_text},
    style::{Marks, Symbol},
};

/// Hash prefix.
const SHA_PREFIX: &str = "sha256:";
/// Content key.
const CONTENT: &str = "content";
/// Permissions key.
const PERMISSIONS: &str = "permissions";

/// One document to expand.
pub struct DetailOption {
    /// The header symbol.
    pub symbol: Symbol,
    /// The header label.
    pub label: String,
    /// The member routes.
    pub routes: Vec<Route>,
    /// Whether members nest under a group branch.
    pub grouped: bool,
}

impl DetailOption {
    /// Collects the detail options.
    pub fn collect(view: &View) -> Vec<Self> {
        let mut out = Vec::new();
        for document in Docs::collect(view) {
            match document {
                Docs::Group { group, members } => {
                    let changes = Docs::changes(&members);
                    let label = preview_group_text(&changes, &group.display());
                    let symbol = Symbol::from_members(&changes);
                    let routes = changes.iter().map(|change| change.path.clone()).collect();
                    out.push(Self {
                        symbol,
                        label,
                        routes,
                        grouped: true,
                    });
                }
                Docs::Doc { change } => {
                    let name = change.path.display();
                    out.push(Self {
                        symbol: row_symbol(change),
                        label: row_text(view, change, &name),
                        routes: vec![change.path.clone()],
                        grouped: false,
                    });
                }
                Docs::Dir { .. } => {}
            }
        }
        out
    }

    /// Builds the detail lines.
    pub fn render(view: &View, selected: &[Self]) -> Vec<Line> {
        let mut blocks = Vec::new();
        for option in selected {
            if option.grouped {
                let mut inner = Vec::new();
                for route in &option.routes {
                    let Some(found) = view.find(route) else {
                        continue;
                    };
                    let name = route.display();
                    inner.push(DisplayBlock::branch(
                        Marks::plan(row_symbol(found)),
                        row_text(view, found, &name),
                        diff_leaves(view, route),
                    ));
                }
                blocks.push(DisplayBlock::branch(
                    Marks::plan(option.symbol),
                    option.label.clone(),
                    inner,
                ));
            } else {
                let Some(path) = option.routes.first() else {
                    continue;
                };
                blocks.push(DisplayBlock::branch(
                    Marks::plan(option.symbol),
                    option.label.clone(),
                    diff_leaves(view, path),
                ));
            }
        }
        DisplayBlock::render(&blocks)
    }
}

impl Display for DetailOption {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> Result {
        write!(formatter, "{} {}", self.symbol, self.label)
    }
}

/// Builds the diff blocks.
fn diff_leaves(view: &View, path: &Route) -> Vec<DisplayBlock> {
    let Some(found) = view.find(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if has_permissions(view, found)
        && let Some(leaf) = permissions_leaf(
            found.before.map(|side| side.mode),
            found.after.map(|side| side.mode),
        )
    {
        out.push(leaf);
    }
    match view.detail(path) {
        Some(ContentDiff::Structured { keys }) => {
            for key in &keys {
                if let Some(leaf) = key_leaf(key) {
                    out.push(leaf);
                }
            }
        }
        Some(ContentDiff::Text { lines }) => {
            for line in &lines {
                out.push(text_leaf(line));
            }
        }
        Some(ContentDiff::Opaque { before, after }) => {
            if let Some(leaf) = opaque_leaf(before, after) {
                out.push(leaf);
            }
        }
        None => {}
    }
    out
}

/// Returns whether one change shows permissions.
fn has_permissions(view: &View, found: &Change) -> bool {
    if found.group.is_some() {
        return true;
    }
    matches!(
        view.find_document(&found.path)
            .map(|document| &document.data),
        Some(Data::Text(_) | Data::Opaque(_))
    )
}

/// Builds one permissions block.
fn permissions_leaf(before: Option<FileMode>, after: Option<FileMode>) -> Option<DisplayBlock> {
    let (symbol, label) = match (before, after) {
        (Some(before), Some(after)) if before != after => (
            Symbol::Update,
            format!("{PERMISSIONS} = {} -> {}", rwx(before), rwx(after)),
        ),
        (Some(_), Some(_)) => return None,
        (Some(before), None) => (Symbol::Remove, format!("{PERMISSIONS} = {}", rwx(before))),
        (None, Some(after)) => (Symbol::Add, format!("{PERMISSIONS} = {}", rwx(after))),
        (None, None) => return None,
    };
    Some(DisplayBlock::leaf(Marks::plan(symbol), label))
}

/// Returns the rwx text.
fn rwx(mode: FileMode) -> String {
    let bits = mode.bits() & 0o777;
    let mut out = String::with_capacity(9);
    for shift in [6, 3, 0] {
        let trio = (bits >> shift) & 0o7;
        out.push(if trio & 0o4 != 0 { 'r' } else { '-' });
        out.push(if trio & 0o2 != 0 { 'w' } else { '-' });
        out.push(if trio & 0o1 != 0 { 'x' } else { '-' });
    }
    out
}

/// Builds one structured key block.
fn key_leaf(key: &KeyDiff) -> Option<DisplayBlock> {
    let (symbol, label) = match (&key.before, &key.after) {
        (Some(before), Some(after)) => (
            Symbol::Update,
            format!(
                "{} = {} -> {}",
                key.key,
                serde_json::to_string(before).unwrap_or_default(),
                serde_json::to_string(after).unwrap_or_default()
            ),
        ),
        (Some(before), None) => (
            Symbol::Remove,
            format!(
                "{} = {}",
                key.key,
                serde_json::to_string(before).unwrap_or_default()
            ),
        ),
        (None, Some(after)) => (
            Symbol::Add,
            format!(
                "{} = {}",
                key.key,
                serde_json::to_string(after).unwrap_or_default()
            ),
        ),
        (None, None) => return None,
    };
    Some(DisplayBlock::leaf(Marks::plan(symbol), label))
}

/// Builds one text line block.
fn text_leaf(line: &LineDiff) -> DisplayBlock {
    let symbol = match line.tag {
        LineTag::Added => Symbol::Add,
        LineTag::Removed => Symbol::Remove,
    };
    DisplayBlock::leaf(Marks::plan(symbol), line.text.clone())
}

/// Builds one opaque content block.
fn opaque_leaf(before: Option<Sha>, after: Option<Sha>) -> Option<DisplayBlock> {
    let (symbol, label) = match (before, after) {
        (Some(before), Some(after)) if before != after => (
            Symbol::Update,
            format!("{CONTENT} = {SHA_PREFIX}{before} -> {SHA_PREFIX}{after}"),
        ),
        (Some(_), Some(_)) => return None,
        (Some(before), None) => (Symbol::Remove, format!("{CONTENT} = {SHA_PREFIX}{before}")),
        (None, Some(after)) => (Symbol::Add, format!("{CONTENT} = {SHA_PREFIX}{after}")),
        (None, None) => return None,
    };
    Some(DisplayBlock::leaf(Marks::plan(symbol), label))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use confit_core::resource_ref::ResourceRef;
    use confit_core::sha::Sha;
    use confit_differ::content::KeyDiff;
    use confit_differ::diff::Diff;
    use confit_model::document::{
        Data, Document, FileMode, LinkDocument, MemberKey, OpaqueDocument, StructuredDocument,
        StructuredFormat, TextDocument, TreeDocument, TreeMember,
    };
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};

    use super::*;

    /// Builds one literal route.
    fn literal(path: &str) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    /// Builds one text document.
    fn text(route: Route, content: &str, mode: FileMode) -> Document {
        Document::new(
            route,
            false,
            Data::Text(TextDocument {
                content: content.to_owned(),
                mode,
            }),
        )
    }

    /// Builds one opaque document.
    fn opaque(route: Route, bytes: &[u8], mode: FileMode) -> Document {
        Document::new(
            route,
            false,
            Data::Opaque(OpaqueDocument {
                handle: ResourceRef::new(Sha::hash(bytes)),
                mode,
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

    /// Builds one manifest from documents.
    fn manifest(documents: Vec<Document>) -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents,
            hooks: Vec::new(),
        }
    }

    #[test]
    fn single_mode_change_marks_mode_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note").display().to_string();
        let wanted = manifest(vec![text(literal(&path), "same", FileMode::new(0o755))]);
        let recorded = manifest(vec![text(literal(&path), "same", FileMode::default())]);
        let diff = Diff::build(wanted, recorded).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        assert!(!options[0].grouped);
        assert_eq!(options[0].routes.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mark.symbol, Some(Symbol::Update));
        assert_eq!(found[0].children.len(), 0);
    }

    #[test]
    fn single_addition_marks_permissions_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note").display().to_string();
        let wanted = manifest(vec![text(literal(&path), "new", FileMode::new(0o755))]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert!(!found.is_empty());
        assert_eq!(found[0].mark.symbol, Some(Symbol::Add));
    }

    #[test]
    fn single_removal_marks_permissions_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note").display().to_string();
        let recorded = manifest(vec![text(literal(&path), "old", FileMode::new(0o755))]);
        let diff = Diff::build(manifest(Vec::new()), recorded).unwrap();
        let options = DetailOption::collect(&diff.drift);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.drift, &options[0].routes[0]);
        assert!(!found.is_empty());
        assert_eq!(found[0].mark.symbol, Some(Symbol::Remove));
    }

    #[test]
    fn added_link_skips_permissions_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("current").display().to_string();
        let target = Route::new(RouteBase::Literal, "new-target").unwrap();
        let wanted = manifest(vec![Document::new(
            literal(&path),
            false,
            Data::Link(LinkDocument { target }),
        )]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert!(!found.is_empty());
    }

    #[test]
    fn single_structured_change_marks_key_leaves() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json").display().to_string();
        let table = |pairs: &[(&str, serde_json::Value)]| {
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let wanted = manifest(vec![Document::new(
            literal(&path),
            false,
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data: table(&[
                    ("same", serde_json::json!(1)),
                    ("edit", serde_json::json!(2)),
                    ("fresh", serde_json::json!("new")),
                ]),
            }),
        )]);
        let recorded = manifest(vec![Document::new(
            literal(&path),
            false,
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data: table(&[
                    ("same", serde_json::json!(1)),
                    ("edit", serde_json::json!(1)),
                    ("stale", serde_json::json!("old")),
                ]),
            }),
        )]);
        let diff = Diff::build(wanted, recorded).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert_eq!(found.len(), 3);
        assert_eq!(found[0].mark.symbol, Some(Symbol::Update));
        assert_eq!(found[1].mark.symbol, Some(Symbol::Add));
        assert_eq!(found[2].mark.symbol, Some(Symbol::Remove));
        assert_eq!(found[0].children.len(), 0);
    }

    #[test]
    fn single_text_change_marks_added_and_removed_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note").display().to_string();
        let wanted = manifest(vec![text(
            literal(&path),
            "one\ntwo\n",
            FileMode::default(),
        )]);
        let recorded = manifest(vec![text(
            literal(&path),
            "one\nthree\n",
            FileMode::default(),
        )]);
        let diff = Diff::build(wanted, recorded).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].mark.symbol, Some(Symbol::Remove));
        assert_eq!(found[1].mark.symbol, Some(Symbol::Add));
    }

    #[test]
    fn single_opaque_change_marks_content_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob").display().to_string();
        let wanted = manifest(vec![opaque(literal(&path), b"after", FileMode::default())]);
        let recorded = manifest(vec![opaque(literal(&path), b"before", FileMode::default())]);
        let diff = Diff::build(wanted, recorded).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mark.symbol, Some(Symbol::Update));
    }

    #[test]
    fn single_opaque_mode_only_change_skips_content_leaf() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blob").display().to_string();
        let wanted = manifest(vec![opaque(literal(&path), b"same", FileMode::new(0o755))]);
        let recorded = manifest(vec![opaque(literal(&path), b"same", FileMode::default())]);
        let diff = Diff::build(wanted, recorded).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = diff_leaves(&diff.desired, &options[0].routes[0]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].mark.symbol, Some(Symbol::Update));
    }

    #[test]
    fn empty_opaque_and_key_pairs_skip_leaves() {
        assert!(opaque_leaf(None, None).is_none());
        assert!(permissions_leaf(None, None).is_none());
        assert!(permissions_leaf(Some(FileMode::default()), Some(FileMode::default())).is_none());
        let key = KeyDiff {
            key: "gone".to_owned(),
            before: None,
            after: None,
        };
        assert!(key_leaf(&key).is_none());
    }

    #[test]
    fn incomparable_row_renders_bare_branch() {
        let dir = tempfile::tempdir().unwrap();
        let route = literal(&dir.path().join("file").display().to_string());
        std::fs::create_dir(dir.path().join("file")).unwrap();
        let previous = manifest(vec![text(route, "recorded", FileMode::default())]);
        let diff = Diff::build(manifest(Vec::new()), previous).unwrap();
        let options = DetailOption::collect(&diff.effective);
        assert_eq!(options.len(), 1);
        assert_eq!(options[0].symbol, Symbol::Query);
        assert!(!options[0].grouped);
        let found = DetailOption::render(&diff.effective, &options);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].marks.symbol, Some(Symbol::Query));
        assert_eq!(found[0].depth, 1);
    }

    #[test]
    fn empty_selection_renders_no_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note").display().to_string();
        let wanted = manifest(vec![text(literal(&path), "new", FileMode::default())]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        let found = DetailOption::render(&diff.desired, &[]);
        assert!(found.is_empty());
        let absent = literal(&dir.path().join("absent").display().to_string());
        assert!(diff_leaves(&diff.desired, &absent).is_empty());
    }

    #[test]
    fn group_option_nests_member_branches() {
        let dir = tempfile::tempdir().unwrap();
        let group = Route::new(RouteBase::Literal, dir.path().join("shared")).unwrap();
        let wanted = manifest(vec![tree(
            group,
            &[("one.txt", b"one"), ("two.txt", b"two")],
        )]);
        let diff = Diff::build(wanted, manifest(Vec::new())).unwrap();
        let options = DetailOption::collect(&diff.desired);
        assert_eq!(options.len(), 1);
        assert!(options[0].grouped);
        assert_eq!(options[0].routes.len(), 2);
        let found = DetailOption::render(&diff.desired, &options);
        assert_eq!(found.len(), 7);
        assert_eq!(found[0].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[0].depth, 1);
        assert_eq!(found[1].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[1].depth, 2);
        assert_eq!(found[2].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[2].depth, 3);
        assert_eq!(found[3].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[3].depth, 3);
        assert_eq!(found[4].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[4].depth, 2);
        assert_eq!(found[5].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[5].depth, 3);
        assert_eq!(found[6].marks.symbol, Some(Symbol::Add));
        assert_eq!(found[6].depth, 3);
    }
}
