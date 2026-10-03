//! Path expr
//!
//! Patch path parsing and JSON flattening.

use serde_json::Value as Json;

use crate::error::{EngineError, PathFault, Scope};

/// One parsed patch path segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Segment {
    /// Table key naming the level.
    pub(crate) key: String,
    /// List index for indexed keys, zero based.
    pub(crate) index: Option<usize>,
}

/// Parses one dotted patch path with single indices.
///
/// Paths count list positions from 1 and store the zero based
/// index, so `servers[1]` holds index 0.
///
/// # Errors
///
/// - [`EngineError::BadPath`] for empty paths, empty
///   segments, and bad indices.
pub(crate) fn parse_path(path: &str, scope: &Scope) -> mlua::Result<Vec<Segment>> {
    if path.is_empty() {
        return Err(bad(path, scope, PathFault::Empty));
    }
    let mut segments = Vec::new();
    for part in path.split('.') {
        if part.is_empty() {
            return Err(bad(path, scope, PathFault::EmptySegment));
        }
        let (key, index) = match part.find('[') {
            None => {
                if part.contains(']') {
                    return Err(bad(path, scope, PathFault::BadIndex));
                }
                (part.to_string(), None)
            }
            Some(open) => {
                let key = part[..open].to_string();
                if key.is_empty() || !part.ends_with(']') || part[open + 1..].contains('[') {
                    return Err(bad(path, scope, PathFault::BadIndex));
                }
                let inner = &part[open + 1..part.len() - 1];
                if inner.is_empty() || !inner.chars().all(|item| item.is_ascii_digit()) {
                    return Err(bad(path, scope, PathFault::BadIndex));
                }
                let number: usize = match inner.parse() {
                    Ok(number) if number >= 1 => number,
                    _ => {
                        return Err(bad(path, scope, PathFault::BadIndex));
                    }
                };
                (key, Some(number - 1))
            }
        };
        segments.push(Segment { key, index });
    }
    Ok(segments)
}

/// Builds one bad path error for its scope.
fn bad(path: &str, scope: &Scope, fault: PathFault) -> mlua::Error {
    EngineError::BadPath {
        scope: scope.clone(),
        path: path.to_owned(),
        fault,
    }
    .into()
}

/// Flattens JSON into dotted leaf entries.
///
/// Array positions echo from 1, so printed keys match typed paths.
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
                flatten_json(item, &format!("{prefix}[{}]", index + 1), out);
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
        let scope = Scope::method("ctx");
        let parsed = match parse_path("a.b[1]", &scope) {
            Ok(parsed) => parsed,
            Err(error) => panic!("path parses: {error}"),
        };
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].index, Some(0));
        assert!(parse_path("", &scope).is_err());
        assert!(parse_path("a..b", &scope).is_err());
        assert!(parse_path("a[1][2]", &scope).is_err());
    }

    #[test]
    fn patch_paths_reject_zero_naming_path() {
        match parse_path("servers[0].host", &Scope::method("ctx")) {
            Ok(_) => panic!("zero index passes"),
            Err(error) => assert!(
                error.to_string().contains("servers[0].host"),
                "zero names the path: {error}"
            ),
        }
    }

    #[test]
    fn flatten_json_counts_arrays_from_one() {
        let value = serde_json::json!({"servers": [{"host": "a"}, {"host": "b"}]});
        let mut out = std::collections::BTreeMap::new();
        flatten_json(&value, "", &mut out);
        assert_eq!(out.get("servers[1].host"), Some(&serde_json::json!("a")));
        assert_eq!(out.get("servers[2].host"), Some(&serde_json::json!("b")));
        assert!(
            !out.keys().any(|key| key.contains("[0]")),
            "no zero key leaks: {:?}",
            out.keys().collect::<Vec<_>>()
        );
    }
}
