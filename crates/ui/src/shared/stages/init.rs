//! Init tree and count line.

use std::path::{Component, Path, PathBuf};

use crate::shared::components::{
    blocks::DisplayBlock,
    style::{Marks, Symbol, strExt as _},
};

/// Created tree title.
pub const CREATED: &str = "Created";

/// Builds the created tree with its count line.
pub(crate) fn init_results(root: &Path, files: &[PathBuf]) -> Vec<String> {
    let mut children = Vec::new();
    let mut seen = Vec::new();
    for file in files {
        if seen.contains(&file) {
            continue;
        }
        seen.push(file);
        let parts = relative_parts(root, file);
        if parts.is_empty() {
            continue;
        }
        let (dirs, leaf) = split_parts(&parts);
        insert_leaf(
            &mut children,
            &dirs,
            DisplayBlock::leaf(Marks::plan(Symbol::Add), leaf),
        );
    }
    sort_blocks(&mut children);
    let mut out = vec![String::new(), CREATED.title(), String::new()];
    out.push(root.display().to_string());
    out.extend(
        DisplayBlock::render(&children)
            .iter()
            .map(|line| line.paint()),
    );
    out.push(count_line(seen.len()));
    out
}

/// Builds the count line with singular and plural forms.
fn count_line(files: usize) -> String {
    if files == 1 {
        "wrote 1 file".to_owned()
    } else {
        format!("wrote {files} files")
    }
}

/// Splits directory parts from the leaf name.
fn split_parts(parts: &[String]) -> (Vec<String>, String) {
    match parts.split_last() {
        Some((leaf, dirs)) => (dirs.to_vec(), leaf.clone()),
        None => (Vec::new(), String::new()),
    }
}

/// Returns the path parts relative to the root.
fn relative_parts(root: &Path, file: &Path) -> Vec<String> {
    if let Ok(relative) = file.strip_prefix(root) {
        let parts = name_parts(relative);
        if !parts.is_empty() {
            return parts;
        }
    }
    file.file_name()
        .map(|name| vec![name.to_string_lossy().into_owned()])
        .unwrap_or_default()
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

/// Sorts branches before leaves with names in order.
fn sort_blocks(blocks: &mut [DisplayBlock]) {
    for block in blocks.iter_mut() {
        sort_blocks(&mut block.children);
    }
    blocks.sort_by(|left, right| {
        let left_branch = left.mark.symbol.is_none();
        let right_branch = right.mark.symbol.is_none();
        match (left_branch, right_branch) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => left.text.cmp(&right.text),
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::init_results;

    #[test]
    fn init_results_sorts_branches_before_leaves() {
        let root = PathBuf::from("site");
        let files = vec![
            root.join("main.lua"),
            root.join("stubs/confit.d.lua"),
            root.join("plugins/_/sugar/init.lua"),
            root.join(".luarc.json"),
        ];
        let lines = init_results(&root, &files);
        assert!(lines.iter().any(|line| line.contains("Created")));
        let text = lines.join("\n");
        assert!(text.contains("main.lua"));
        assert!(text.contains("stubs"));
        assert!(text.contains("confit.d.lua"));
        assert!(text.contains("plugins"));
        assert!(lines.iter().any(|line| line == "wrote 4 files"));
        let plugins = text.find("plugins").unwrap();
        let stubs = text.find("stubs").unwrap();
        let main = text.find("main.lua").unwrap();
        assert!(plugins < stubs);
        assert!(stubs < main);
        assert!(
            lines.iter().any(|line| line == "site"),
            "the root shows as a bare path line"
        );
    }

    #[test]
    fn init_results_writes_singular_for_one_file() {
        let root = PathBuf::from("site");
        let files = vec![root.join(".luarc.json")];
        let lines = init_results(&root, &files);
        assert!(lines.iter().any(|line| line == "wrote 1 file"));
    }

    #[test]
    fn init_results_falls_back_to_file_name_outside_root() {
        let root = PathBuf::from("site");
        let files = vec![PathBuf::from("/elsewhere/main.lua")];
        let lines = init_results(&root, &files);
        let text = lines.join("\n");
        assert!(text.contains("main.lua"));
        assert!(lines.iter().any(|line| line == "wrote 1 file"));
    }
}
