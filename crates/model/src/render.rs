//! Render
//! Inline payloads to portable on-disk bytes.

use crate::arg::{Arg, quote};
use crate::condition::Condition;
use crate::document::{Data, RcData, RcEntry, StructuredFormat};
use crate::error::{Error, Result};

/// Interactivity guard shared by every shell file.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

/// Render inline payload bytes in portable display form.
///
/// # Errors
///
/// - [`Error::Parse`] for opaque and tree documents.
/// - [`Error::Render`] for serializer failures.
pub fn inline_bytes(data: &Data) -> Result<Vec<u8>> {
    match data {
        Data::Structured { format, data } => {
            let ordered = canonical_data(data);
            match format {
                StructuredFormat::Toml => Ok(render_toml(&ordered)?.into_bytes()),
                StructuredFormat::Json => Ok(render_json(&ordered)?.into_bytes()),
                StructuredFormat::Yaml => Ok(render_yaml(&ordered)?.into_bytes()),
            }
        }
        Data::Text { content, .. } => Ok(content.as_bytes().to_vec()),
        Data::Link { target } => Ok(target.display().as_bytes().to_vec()),
        Data::Rc(data) => Ok(render_rc(data).into_bytes()),
        Data::Opaque { handle: blob, .. } => Err(Error::Parse {
            input: blob.sha256().hex(),
            want: "blob bytes through the blob store".to_owned(),
        }),
        Data::Tree { .. } => Err(Error::Parse {
            input: "tree".to_owned(),
            want: "member bytes for tree documents".to_owned(),
        }),
    }
}

/// Render a value to TOML text.
fn render_toml(value: &serde_json::Value) -> Result<String> {
    toml::to_string(value).map_err(|source| Error::Render {
        format: StructuredFormat::Toml,
        reason: source.to_string(),
    })
}

/// Render a value to pretty JSON text.
fn render_json(value: &serde_json::Value) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|source| Error::Render {
        format: StructuredFormat::Json,
        reason: source.to_string(),
    })
}

/// Render a value to YAML text.
fn render_yaml(value: &serde_json::Value) -> Result<String> {
    noyalib::to_string(value).map_err(|source| Error::Render {
        format: StructuredFormat::Yaml,
        reason: source.to_string(),
    })
}

/// Sort one structured value into canonical key order.
///
/// - Objects sort by key at every depth.
/// - Arrays keep declaration order.
fn canonical_data(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut pairs: Vec<(&String, &serde_json::Value)> = map.iter().collect();
            pairs.sort_by(|left, right| left.0.cmp(right.0));
            let mut sorted = serde_json::Map::new();
            for (key, item) in pairs {
                sorted.insert(key.clone(), canonical_data(item));
            }
            serde_json::Value::Object(sorted)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_data).collect())
        }
        _ => value.clone(),
    }
}

/// Render rc data to shell text with trailing newline.
fn render_rc(data: &RcData) -> String {
    let profile = section_lines(&data.profile);
    let config = section_lines(&data.config);
    let finals = section_lines(&data.final_entries);
    let mut blocks: Vec<String> = Vec::new();
    if !profile.is_empty() {
        blocks.push(profile.join("\n"));
    }
    if !data.config.is_empty() || !data.final_entries.is_empty() {
        blocks.push(GUARD.to_string());
    }
    if !config.is_empty() {
        blocks.push(config.join("\n"));
    }
    if !finals.is_empty() {
        blocks.push(finals.join("\n"));
    }
    if blocks.is_empty() {
        return String::new();
    }
    let mut out = blocks.join("\n\n");
    out.push('\n');
    out
}

/// Collect one section lines in declaration order.
fn section_lines(entries: &[RcEntry]) -> Vec<String> {
    entries.iter().flat_map(render_entry).collect()
}

/// Render one rc entry as shell lines.
fn render_entry(entry: &RcEntry) -> Vec<String> {
    let line = match entry {
        RcEntry::Env { name, value, .. } => {
            let exported = escape_argv(std::slice::from_ref(value));
            format!("export {name}={exported}")
        }
        RcEntry::Path { name, dir, .. } => {
            let expanded = dir.display();
            let placed = quote(&expanded);
            format!("export {name}={placed}:\"${{{name}}}\"")
        }
        RcEntry::Alias {
            name, expansion, ..
        } => {
            let expanded = escape_argv(std::slice::from_ref(expansion));
            format!("alias {name}={expanded}")
        }
        RcEntry::Eval { argv, .. } => format!("eval \"$({})\"", escape_args(argv)),
        RcEntry::Cmd { argv, .. } => escape_args(argv),
        RcEntry::Source { path, .. } => {
            let expanded = path.display();
            format!("source {}", quote(&expanded))
        }
    };
    match entry.when() {
        None => vec![line],
        Some(guard) => vec![
            format!("if {}; then", render_guard(guard)),
            format!("  {line}"),
            "fi".to_string(),
        ],
    }
}

/// Render one condition as a Bourne test string.
fn render_guard(guard: &Condition) -> String {
    match guard {
        Condition::EnvEq { key, value } => {
            let rhs = escape_argv(std::slice::from_ref(value));
            format!("[ \"${{{key}}}\" = {rhs} ]")
        }
        Condition::EnvSet { key } => format!("[ -n \"${{{key}}}\" ]"),
        Condition::InPath { name } => {
            let binary = escape_argv(std::slice::from_ref(name));
            format!("command -v {binary} >/dev/null 2>&1")
        }
        Condition::Exists { route } => {
            let candidate = escape_argv(std::slice::from_ref(&route.display()));
            format!("[ -e {candidate} ]")
        }
        // Changed renders false.
        Condition::Changed { .. } => "false".to_string(),
        Condition::All(items) if items.is_empty() => "true".to_string(),
        Condition::Any(items) if items.is_empty() => "false".to_string(),
        Condition::All(items) => join_guards(items, "&&"),
        Condition::Any(items) => join_guards(items, "||"),
        Condition::Not(inner) => match inner.as_ref() {
            Condition::All(_) | Condition::Any(_) | Condition::Not(_) => {
                format!("! ( {} )", render_guard(inner))
            }
            _ => format!("! {}", render_guard(inner)),
        },
    }
}

/// Join nested guards under one operator.
fn join_guards(items: &[Condition], op: &str) -> String {
    items
        .iter()
        .map(|item| group_guard(item, op))
        .collect::<Vec<_>>()
        .join(&format!(" {op} "))
}

/// Group one nested guard for joining under an operator.
fn group_guard(item: &Condition, parent: &str) -> String {
    let rendered = render_guard(item);
    match item {
        Condition::All(_) if parent != "&&" => format!("( {rendered} )"),
        Condition::Any(_) if parent != "||" => format!("( {rendered} )"),
        _ => rendered,
    }
}

/// Render argv as one shell line with Bourne quoting.
fn escape_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote(arg).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render slots as one shell line with Bourne quoting.
fn escape_args(slots: &[Arg]) -> String {
    slots.iter().map(escape_slot).collect::<Vec<_>>().join(" ")
}

/// Render one slot with Bourne quoting.
fn escape_slot(slot: &Arg) -> String {
    match slot {
        Arg::Text(text) => quote(text).into_owned(),
        Arg::Route(route) => {
            let expanded = route.display();
            quote(&expanded).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::StructuredFormat;

    fn payload(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    fn structured(format: StructuredFormat, data: serde_json::Value) -> Data {
        Data::Structured { format, data }
    }

    #[test]
    fn structured_bytes_ignore_key_order() {
        let first = payload(r#"{"beta": {"delta": 1, "gamma": [1, 2]}, "alpha": "one"}"#);
        let second = payload(r#"{"alpha": "one", "beta": {"gamma": [1, 2], "delta": 1}}"#);
        for format in [
            StructuredFormat::Json,
            StructuredFormat::Toml,
            StructuredFormat::Yaml,
        ] {
            let one = inline_bytes(&structured(format, first.clone())).unwrap();
            let two = inline_bytes(&structured(format, second.clone())).unwrap();
            assert_eq!(one, two, "key order leaves structured bytes steady");
        }
        let changed = payload(r#"{"alpha": "two", "beta": {"gamma": [1, 2], "delta": 1}}"#);
        let before = inline_bytes(&structured(StructuredFormat::Json, first.clone())).unwrap();
        let after = inline_bytes(&structured(StructuredFormat::Json, changed)).unwrap();
        assert_ne!(before, after, "content change moves structured bytes");
        let flipped = payload(r#"{"alpha": "one", "beta": {"gamma": [2, 1], "delta": 1}}"#);
        let reordered = inline_bytes(&structured(StructuredFormat::Json, flipped)).unwrap();
        assert_ne!(before, reordered, "array order moves structured bytes");
    }
}
