//! Tagged text line diffs.

use crate::content::{LineDiff, LineTag};

/// Diffs two texts into added and removed lines.
pub(crate) fn diff(before: &str, after: &str) -> Vec<LineDiff> {
    let mut out = Vec::new();
    for line in diffy::create_patch(before, after).to_string().lines() {
        if let Some(found) = tagged(line) {
            out.push(found);
        }
    }
    out
}

/// Tags one patch line.
fn tagged(line: &str) -> Option<LineDiff> {
    if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
        return None;
    }
    if line.starts_with('\\') {
        return None;
    }
    let (tag, text) = match line.as_bytes().first()? {
        b'+' => (LineTag::Added, &line[1..]),
        b'-' => (LineTag::Removed, &line[1..]),
        _ => return None,
    };
    Some(LineDiff {
        tag,
        text: text.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_tags_added_and_removed_lines() {
        let found = diff("one\ntwo\n", "one\nthree\n");
        assert_eq!(
            found,
            vec![
                LineDiff {
                    tag: LineTag::Removed,
                    text: "two".to_owned(),
                },
                LineDiff {
                    tag: LineTag::Added,
                    text: "three".to_owned(),
                },
            ],
            "only changed lines surface with tags"
        );
    }

    #[test]
    fn line_diff_shows_only_changed_lines() {
        assert!(
            diff("same\n", "same\n").is_empty(),
            "equal texts produce empty diffs"
        );
    }
}
