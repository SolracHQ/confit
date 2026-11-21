//! Display blocks

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Component, Path, PathBuf},
};

use confit_differ::change::{Change, Comparability};
use confit_model::routes::Route;

use crate::shared::components::style::{Marks, Outcome, strExt as _};

/// One block of the rendered tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayBlock {
    /// Block marks.
    pub mark: Marks,
    /// Plain block text without glyphs.
    pub text: String,
    /// Child blocks.
    pub children: Vec<DisplayBlock>,
}

/// One rendered line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Line marks.
    pub marks: Marks,
    /// Plain line label without glyphs.
    pub label: String,
    /// Nesting depth with forest roots at one.
    pub depth: usize,
    /// Glyph prefix for the marks.
    pub prefix: String,
}

impl DisplayBlock {
    /// Builds one leaf block.
    pub fn leaf(mark: Marks, text: String) -> Self {
        Self {
            mark,
            text,
            children: Vec::new(),
        }
    }

    /// Builds one branch block.
    pub fn branch(mark: Marks, text: String, children: Vec<Self>) -> Self {
        Self {
            mark,
            text,
            children,
        }
    }

    /// Sets the outcome, keeping the symbol.
    pub fn apply(&mut self, outcome: Outcome) {
        self.mark.outcome = Some(outcome);
    }

    /// Builds lines without a root.
    pub fn render(forest: &[Self]) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut stack: Vec<(&Self, usize, String, bool)> = Vec::new();
        let total = forest.len();
        for (index, child) in forest.iter().enumerate().rev() {
            let last = index + 1 == total;
            stack.push((child, 1, String::new(), last));
        }
        while let Some((block, depth, indent, last)) = stack.pop() {
            let stem = if last { "└── " } else { "├── " };
            let prefix = format!("{indent}{stem}");
            let next = format!("{indent}{}", if last { "    " } else { "│   " });
            lines.push(Line {
                marks: block.mark,
                label: block.text.clone(),
                depth,
                prefix,
            });
            let total = block.children.len();
            for (index, child) in block.children.iter().enumerate().rev() {
                let last = index + 1 == total;
                stack.push((child, depth + 1, next.clone(), last));
            }
        }
        lines
    }
}

impl Line {
    /// Returns the painted line.
    pub fn paint(&self) -> String {
        match (self.marks.outcome, self.marks.symbol) {
            (None, None) => {
                if self.depth == 0 {
                    self.label.header()
                } else {
                    format!("{}{}", self.prefix, self.label)
                }
            }
            (None, Some(symbol)) => {
                format!("{}{symbol} {}", self.prefix, self.label)
            }
            (Some(outcome), symbol) => {
                let painted = match symbol {
                    Some(found) => format!("{}{outcome} {found} {}", self.prefix, self.label),
                    None => format!("{}{outcome} {}", self.prefix, self.label),
                };
                if outcome == Outcome::Unattempted {
                    painted.dim()
                } else {
                    painted
                }
            }
        }
    }
}

/// Grouped documents for block builders.
#[derive(Debug)]
pub(crate) enum Docs<'a> {
    /// One directory level in the factored tree.
    Dir {
        /// Directory name.
        name: String,
        /// Child documents.
        children: Vec<Docs<'a>>,
    },
    /// One tree group section.
    Group {
        /// Tree route for the members.
        group: &'a Route,
        /// Member documents.
        members: Vec<Docs<'a>>,
    },
    /// Ungrouped row.
    Doc {
        /// Member row.
        change: &'a Change,
    },
}

impl<'a> Docs<'a> {
    /// Collects the top-level documents.
    pub(crate) fn collect(rows: &'a [Change]) -> Vec<Docs<'a>> {
        let members = Self::members_by_group(rows);
        let mut seen: BTreeMap<&Route, ()> = BTreeMap::new();
        let mut documents = Vec::new();
        for change in rows {
            match change.group.as_ref() {
                Some(group) if Self::comparable(change) => {
                    if seen.insert(group, ()).is_none() {
                        let empty: &[&Change] = &[];
                        let found = members.get(group).map(Vec::as_slice).unwrap_or(empty);
                        let mut grouped = Vec::new();
                        for member in found.iter().copied() {
                            grouped.push(Docs::Doc { change: member });
                        }
                        documents.push(Docs::Group {
                            group,
                            members: grouped,
                        });
                    }
                }
                _ => documents.push(Docs::Doc { change }),
            }
        }
        documents
    }

    /// Groups the documents by ancestor root.
    pub(crate) fn cluster(
        rows: &[&'a Change],
        documents: &'a [Docs<'a>],
    ) -> BTreeMap<PathBuf, Vec<&'a Docs<'a>>> {
        let roots = Self::cluster_roots(rows);
        let mut sections: BTreeMap<PathBuf, Vec<&Docs>> = BTreeMap::new();
        for document in documents {
            let root = Self::document_key(document)
                .and_then(|key| roots.get(&key).cloned())
                .unwrap_or_else(|| PathBuf::from("/"));
            sections.entry(root).or_default().push(document);
        }
        sections
    }

    /// Factors one root section into nested directories.
    pub(crate) fn factor(root: &Path, documents: &[&'a Docs<'a>]) -> Docs<'a> {
        let mut children = Vec::new();
        for document in documents.iter().copied() {
            match document {
                Docs::Group { group, members } => {
                    let base = Self::group_host_path(group);
                    let mut parts = Self::name_parts(&Self::relative(&base, root));
                    if parts.is_empty() {
                        parts.push(Self::leaf_name(&base));
                    }
                    let mut flat = Vec::new();
                    Self::collect_changes(members, &mut flat);
                    Self::insert_group(&mut children, &parts, group, &flat);
                }
                Docs::Doc { change } => {
                    let mut parts = Self::name_parts(&Self::relative(&change.expanded, root));
                    if parts.is_empty() {
                        parts.push(Self::leaf_name(&change.expanded));
                    }
                    Self::insert_single(&mut children, &parts, change);
                }
                Docs::Dir {
                    children: inner, ..
                } => {
                    let mut flat = Vec::new();
                    Self::collect_changes(inner, &mut flat);
                    for change in flat {
                        let mut parts = Self::name_parts(&Self::relative(&change.expanded, root));
                        if parts.is_empty() {
                            parts.push(Self::leaf_name(&change.expanded));
                        }
                        Self::insert_single(&mut children, &parts, change);
                    }
                }
            }
        }
        Docs::Dir {
            name: root.display().to_string(),
            children,
        }
    }

    /// Returns the flat member rows.
    pub(crate) fn rows(members: &[Docs<'a>]) -> Vec<&'a Change> {
        let mut rows = Vec::new();
        Self::collect_changes(members, &mut rows);
        rows
    }

    /// Returns whether the row belongs to its group.
    fn comparable(change: &Change) -> bool {
        matches!(change.comparability, Comparability::Comparable)
    }

    /// Collects the grouped member rows.
    fn members_by_group(rows: &'a [Change]) -> BTreeMap<&'a Route, Vec<&'a Change>> {
        let mut members: BTreeMap<&Route, Vec<&Change>> = BTreeMap::new();
        for change in rows {
            if let Some(group) = change.group.as_ref()
                && Self::comparable(change)
            {
                members.entry(group).or_default().push(change);
            }
        }
        members
    }

    /// Returns the cluster key.
    fn document_key(document: &Docs<'a>) -> Option<OsString> {
        match document {
            Docs::Doc { change } => Some(Self::top_key(&change.expanded)),
            Docs::Group { members, .. } => Self::first_expanded(members).map(Self::top_key),
            Docs::Dir { children, .. } => Self::first_expanded(children).map(Self::top_key),
        }
    }

    /// Returns the first expanded path.
    fn first_expanded<'b>(members: &'b [Docs<'a>]) -> Option<&'b Path> {
        for member in members {
            if let Some(path) = Self::document_expanded(member) {
                return Some(path);
            }
        }
        None
    }

    /// Returns the expanded path.
    fn document_expanded<'b>(document: &'b Docs<'a>) -> Option<&'b Path> {
        match document {
            Docs::Doc { change } => Some(&change.expanded),
            Docs::Group { members, .. } => Self::first_expanded(members),
            Docs::Dir { children, .. } => Self::first_expanded(children),
        }
    }

    /// Groups the paths by ancestor root.
    fn cluster_roots(rows: &[&'a Change]) -> BTreeMap<OsString, PathBuf> {
        let mut groups: BTreeMap<OsString, Vec<&Path>> = BTreeMap::new();
        for change in rows {
            groups
                .entry(Self::top_key(&change.expanded))
                .or_default()
                .push(&change.expanded);
        }
        groups
            .into_iter()
            .map(|(key, paths)| (key, Self::cluster_root(&paths)))
            .collect()
    }

    /// Returns the cluster root.
    fn cluster_root(paths: &[&Path]) -> PathBuf {
        match paths {
            [] => PathBuf::from("/"),
            [only] => only
                .parent()
                .map_or_else(|| PathBuf::from("/"), Path::to_path_buf),
            [first, rest @ ..] => rest.iter().fold(first.to_path_buf(), |current, path| {
                Self::longest_prefix(&current, path)
            }),
        }
    }

    /// Returns the longest shared path prefix.
    fn longest_prefix(first: &Path, second: &Path) -> PathBuf {
        let mut prefix = PathBuf::new();
        for pair in first.components().zip(second.components()) {
            if pair.0 != pair.1 {
                break;
            }
            prefix.push(pair.0);
        }
        prefix
    }

    /// Returns the first normal path component.
    fn top_key(path: &Path) -> OsString {
        path.components()
            .find_map(|part| match part {
                Component::Normal(found) => Some(found.to_os_string()),
                _ => None,
            })
            .unwrap_or_default()
    }

    /// Collects every row under one document list.
    fn collect_changes(members: &[Docs<'a>], output: &mut Vec<&'a Change>) {
        for member in members {
            match member {
                Docs::Doc { change } => output.push(*change),
                Docs::Dir { children, .. } => {
                    Self::collect_changes(children, output);
                }
                Docs::Group { members: inner, .. } => {
                    Self::collect_changes(inner, output);
                }
            }
        }
    }

    /// Inserts one plain file in its parent directories.
    fn insert_single(children: &mut Vec<Docs<'a>>, parts: &[String], change: &'a Change) {
        let (name, parents) = match parts.split_last() {
            Some(found) => found,
            None => return,
        };
        let mut current = children;
        for part in parents {
            current = Self::dir_children_mut(current, part);
        }
        if Self::has_name(current, name) {
            let nested = Self::dir_children_mut(current, name);
            let leaf = Self::leaf_name(&change.expanded);
            if !Self::has_file(nested, &leaf) {
                nested.push(Docs::Doc { change });
            }
            return;
        }
        if !Self::has_file(current, name) {
            current.push(Docs::Doc { change });
        }
    }

    /// Inserts one group.
    fn insert_group(
        children: &mut Vec<Docs<'a>>,
        parts: &[String],
        group: &'a Route,
        members: &[&'a Change],
    ) {
        let (name, parents) = match parts.split_last() {
            Some(found) => found,
            None => return,
        };
        let mut current = children;
        for part in parents {
            current = Self::dir_children_mut(current, part);
        }
        let target = Self::group_members_mut(current, name, group);
        let base = Self::group_host_path(group);
        for change in members.iter().copied() {
            let mut inner = Self::name_parts(&Self::relative(&change.expanded, &base));
            if inner.is_empty() {
                inner.push(Self::leaf_name(&change.expanded));
            }
            Self::insert_member(target, &inner, change);
        }
    }

    /// Inserts one member file in its group directories.
    fn insert_member(members: &mut Vec<Docs<'a>>, parts: &[String], change: &'a Change) {
        let (name, parents) = match parts.split_last() {
            Some(found) => found,
            None => return,
        };
        let mut current = members;
        for part in parents {
            current = Self::dir_children_mut(current, part);
        }
        if !Self::has_file(current, name) {
            current.push(Docs::Doc { change });
        }
    }

    /// Returns the child directory.
    fn dir_children_mut<'b>(children: &'b mut Vec<Docs<'a>>, name: &str) -> &'b mut Vec<Docs<'a>> {
        let mut position = None;
        for (index, child) in children.iter().enumerate() {
            if let Docs::Dir { name: current, .. } = child
                && current == name
            {
                position = Some(index);
                break;
            }
        }
        if position.is_none() {
            children.push(Docs::Dir {
                name: name.to_owned(),
                children: Vec::new(),
            });
            position = Some(children.len() - 1);
        }
        let index = match position {
            Some(found) => found,
            None => unreachable!("missing directory"),
        };
        if let Docs::Dir {
            children: inner, ..
        } = &mut children[index]
        {
            inner
        } else {
            unreachable!("missing directory")
        }
    }

    /// Returns the member list.
    fn group_members_mut<'b>(
        children: &'b mut Vec<Docs<'a>>,
        name: &str,
        group: &'a Route,
    ) -> &'b mut Vec<Docs<'a>> {
        let mut position = None;
        for (index, child) in children.iter().enumerate() {
            if let Docs::Group { group: current, .. } = child
                && Self::group_key(current) == name
            {
                position = Some(index);
                break;
            }
        }
        if position.is_none() {
            children.push(Docs::Group {
                group,
                members: Vec::new(),
            });
            position = Some(children.len() - 1);
        }
        let index = match position {
            Some(found) => found,
            None => unreachable!("missing group"),
        };
        if let Docs::Group { members: inner, .. } = &mut children[index] {
            inner
        } else {
            unreachable!("missing group")
        }
    }

    /// Returns whether the name is taken.
    fn has_name(children: &[Docs<'a>], name: &str) -> bool {
        for child in children {
            match child {
                Docs::Dir { name: current, .. } if current == name => {
                    return true;
                }
                Docs::Group { group, .. } if Self::group_key(group) == name => {
                    return true;
                }
                _ => continue,
            }
        }
        false
    }

    /// Returns whether the file is present.
    fn has_file(children: &[Docs<'a>], name: &str) -> bool {
        for child in children {
            if let Docs::Doc { change } = child
                && Self::leaf_name(&change.expanded) == name
            {
                return true;
            }
        }
        false
    }

    /// Returns the directory key.
    fn group_key(group: &Route) -> String {
        Self::leaf_name(&Self::group_host_path(group))
    }

    /// Returns the normal components.
    fn name_parts(path: &Path) -> Vec<String> {
        path.components()
            .filter_map(|part| match part {
                Component::Normal(found) => Some(found.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect()
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

    /// Strips the root prefix or returns the path itself.
    fn relative(path: &Path, root: &Path) -> PathBuf {
        path.strip_prefix(root)
            .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
    }
}

#[cfg(test)]
mod tests {
    use super::DisplayBlock;
    use crate::shared::components::style::{Marks, Outcome, Symbol};

    #[test]
    fn leaf_builds_block_with_marks_and_text() {
        let block = DisplayBlock::leaf(Marks::plan(Symbol::Add), "one".to_owned());
        assert_eq!(block.mark.symbol, Some(Symbol::Add));
        assert_eq!(block.mark.outcome, None);
        assert_eq!(block.text, "one");
        assert_eq!(block.children.len(), 0);
    }

    #[test]
    fn branch_builds_block_with_children() {
        let first = DisplayBlock::leaf(Marks::plan(Symbol::Add), "one".to_owned());
        let second = DisplayBlock::leaf(Marks::plan(Symbol::Remove), "two".to_owned());
        let block = DisplayBlock::branch(
            Marks {
                outcome: None,
                symbol: None,
            },
            "root".to_owned(),
            vec![first, second],
        );
        assert_eq!(block.text, "root");
        assert_eq!(block.children.len(), 2);
        assert_eq!(block.children[0].text, "one");
        assert_eq!(block.children[1].text, "two");
        assert_eq!(block.children[0].mark.symbol, Some(Symbol::Add));
        assert_eq!(block.children[1].mark.symbol, Some(Symbol::Remove));
    }

    #[test]
    fn branch_nests_blocks_two_levels_deep() {
        let leaf = DisplayBlock::leaf(Marks::plan(Symbol::Add), "leaf".to_owned());
        let inner = DisplayBlock::branch(
            Marks {
                outcome: None,
                symbol: None,
            },
            "inner".to_owned(),
            vec![leaf],
        );
        let outer = DisplayBlock::branch(
            Marks {
                outcome: None,
                symbol: None,
            },
            "outer".to_owned(),
            vec![inner],
        );
        assert_eq!(outer.children.len(), 1);
        assert_eq!(outer.children[0].text, "inner");
        assert_eq!(outer.children[0].children.len(), 1);
        assert_eq!(outer.children[0].children[0].text, "leaf");
        assert_eq!(outer.children[0].children[0].mark.symbol, Some(Symbol::Add));
    }

    #[test]
    fn render_emits_prefixes_and_depths_for_forest() {
        let leaf = DisplayBlock::leaf(Marks::plan(Symbol::Remove), "gone".to_owned());
        let branch = DisplayBlock::branch(Marks::plan(Symbol::Add), "doc".to_owned(), vec![leaf]);
        let found = DisplayBlock::render(&[branch]);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].label, "doc");
        assert_eq!(found[0].depth, 1);
        assert_eq!(found[0].prefix, "└── ");
        assert_eq!(found[1].label, "gone");
        assert_eq!(found[1].depth, 2);
        assert_eq!(found[1].prefix, "    └── ");
        let _ = found[0].paint();
    }

    #[test]
    fn apply_keeps_symbol_and_sets_outcome() {
        let mut block = DisplayBlock::leaf(Marks::plan(Symbol::Add), "one".to_owned());
        block.apply(Outcome::Applied);
        assert_eq!(block.mark.symbol, Some(Symbol::Add));
        assert_eq!(block.mark.outcome, Some(Outcome::Applied));
        assert_eq!(block.text, "one");
        assert_eq!(block.children.len(), 0);
    }
}
