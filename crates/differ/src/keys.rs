//! Flattened structured key diffs.

use std::collections::BTreeMap;

use crate::content::KeyDiff;

/// One parsed structured table keyed by field name.
pub(crate) type Table = BTreeMap<String, serde_json::Value>;

/// Flattens two tables into per-key changes.
pub(crate) fn flatten(before: &Table, after: &Table) -> Vec<KeyDiff> {
    let mut out = Vec::new();
    for key in union_keys(before.keys(), after.keys()) {
        leaf(&key, before.get(&key), after.get(&key), &mut out);
    }
    out
}

/// Collects one leaf change through shared maps and lists.
fn leaf(
    prefix: &str,
    before: Option<&serde_json::Value>,
    after: Option<&serde_json::Value>,
    out: &mut Vec<KeyDiff>,
) {
    if let (Some(held), Some(wanted)) = (before, after) {
        match (held, wanted) {
            (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
                for key in union_keys(left.keys(), right.keys()) {
                    leaf(
                        &format!("{prefix}.{key}"),
                        left.get(&key),
                        right.get(&key),
                        out,
                    );
                }
                return;
            }
            (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
                let len = left.len().max(right.len());
                for index in 0..len {
                    leaf(
                        &format!("{prefix}[{}]", index + 1),
                        left.get(index),
                        right.get(index),
                        out,
                    );
                }
                return;
            }
            _ => {}
        }
    }
    if before != after {
        out.push(KeyDiff {
            key: prefix.to_owned(),
            before: before.cloned(),
            after: after.cloned(),
        });
    }
}

/// Returns the sorted union of two key sets.
fn union_keys<'a>(
    before: impl IntoIterator<Item = &'a String>,
    after: impl IntoIterator<Item = &'a String>,
) -> Vec<String> {
    let mut keys: Vec<String> = before.into_iter().chain(after).cloned().collect();
    keys.sort();
    keys.dedup();
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Builds one string-keyed table from JSON values.
    fn table(pairs: &[(&str, serde_json::Value)]) -> Table {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn flatten_reports_added_changed_and_removed_keys() {
        let before = table(&[("keep", json!(1)), ("change", json!(1)), ("drop", json!(1))]);
        let after = table(&[
            ("keep", json!(1)),
            ("change", json!(2)),
            ("fresh", json!(3)),
        ]);
        let found = flatten(&before, &after);
        assert_eq!(found.len(), 3, "only the differing keys surface");
        assert_eq!(found[0].key, "change");
        assert_eq!(found[0].before, Some(json!(1)));
        assert_eq!(found[0].after, Some(json!(2)));
        assert_eq!(found[1].key, "drop");
        assert_eq!(found[1].after, None);
        assert_eq!(found[2].key, "fresh");
        assert_eq!(found[2].before, None);
    }

    #[test]
    fn flatten_builds_dotted_keys_with_indexes() {
        let before = table(&[("nested", json!({"deep": [1, 2]}))]);
        let after = table(&[("nested", json!({"deep": [1, 3]}))]);
        let found = flatten(&before, &after);
        assert_eq!(found.len(), 1, "one leaf differs");
        assert_eq!(found[0].key, "nested.deep[2]");
        assert_eq!(found[0].before, Some(json!(2)));
        assert_eq!(found[0].after, Some(json!(3)));
    }
}
