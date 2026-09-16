//! Path expr
//!
//! Patch path parsing plus JSON flattening.

use serde_json::Value as Json;

use crate::error::plan_error;

/// One parsed patch path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    /// Table key naming the level.
    pub(crate) key: String,
    /// List index for indexed keys.
    pub(crate) index: Option<usize>,
}

/// Parses one dotted patch path with single indices.
pub(crate) fn parse_path(path: &str, caller: &str) -> mlua::Result<Vec<Segment>> {
    if path.is_empty() {
        return Err(plan_error(format!("{caller}: invalid path '': empty path")));
    }
    let mut segments = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            return Err(plan_error(format!(
                "{caller}: invalid path '{path}': empty segment"
            )));
        }
        let (key, index) = match part.find('[') {
            None => {
                if part.contains(']') {
                    return Err(plan_error(format!(
                        "{caller}: invalid path '{path}': bad index"
                    )));
                }
                (part.to_string(), None)
            }
            Some(open) => {
                let key = part[..open].to_string();
                if key.is_empty() || !part.ends_with(']') || part[open + 1..].contains('[') {
                    return Err(plan_error(format!(
                        "{caller}: invalid path '{path}': bad index"
                    )));
                }
                let inner = &part[open + 1..part.len() - 1];
                if inner.is_empty() || !inner.chars().all(|item| item.is_ascii_digit()) {
                    return Err(plan_error(format!(
                        "{caller}: invalid path '{path}': bad index"
                    )));
                }
                match inner.parse::<usize>() {
                    Ok(index) => (key, Some(index)),
                    Err(_) => {
                        return Err(plan_error(format!(
                            "{caller}: invalid path '{path}': bad index"
                        )));
                    }
                }
            }
        };
        segments.push(Segment { key, index });
    }
    Ok(segments)
}

/// Flattens JSON into dotted leaf entries.
pub(crate) fn flatten_json(
    value: &Json,
    prefix: &str,
    out: &mut std::collections::BTreeMap<String, Json>,
) {
    match value {
        Json::Object(map) => {
            for (key, item) in map {
                let child = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_json(item, &child, out);
            }
        }
        Json::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                flatten_json(item, &format!("{prefix}[{index}]"), out);
            }
        }
        _ => {
            out.insert(prefix.to_string(), value.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_parse_dotted_shapes() {
        let parsed = match parse_path("a.b[0]", "ctx") {
            Ok(parsed) => parsed,
            Err(error) => panic!("path parses: {error}"),
        };
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].index, Some(0));
        assert!(parse_path("", "ctx").is_err());
        assert!(parse_path("a..b", "ctx").is_err());
        assert!(parse_path("a[1][2]", "ctx").is_err());
    }
}
