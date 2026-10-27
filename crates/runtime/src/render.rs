//! Render
//! Document payloads to on-disk bytes.

use std::collections::BTreeMap;

use confit_core::error::{DocumentError, ErrorCause};
use confit_model::arg::{Arg, quote};
use confit_model::condition::Condition;
use confit_model::document::{Data, Document, RcData, RcEntry, StructuredFormat};
use confit_model::routes::Route;

/// Interactivity guard shared by every shell file.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

/// Render one document to exact on-disk bytes.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::OpaqueNotRenderable`] for opaque payloads.
/// - [`confit_core::error::DocumentError::TreeNotRenderable`] for tree payloads.
/// - [`confit_core::error::DocumentError::RenderFailed`] for serializer failures.
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
pub fn render_document(document: &Document) -> std::result::Result<Vec<u8>, DocumentError> {
    match &document.data {
        Data::Structured(found) => match found.format {
            StructuredFormat::Toml => render_toml(&found.data).map(|text| text.into_bytes()),
            StructuredFormat::Json => render_json(&found.data).map(|text| text.into_bytes()),
            StructuredFormat::Yaml => render_yaml(&found.data).map(|text| text.into_bytes()),
        },
        Data::Text(found) => Ok(found.content.as_bytes().to_vec()),
        Data::Link(found) => Ok(found.target.display().as_bytes().to_vec()),
        Data::Rc(data) => render_rc(data).map(|text| text.into_bytes()),
        Data::Opaque(_) => Err(DocumentError::OpaqueNotRenderable),
        Data::Tree(_) => Err(DocumentError::TreeNotRenderable),
    }
}

/// Render one map to TOML text.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] for serializer failures.
fn render_toml(
    value: &BTreeMap<String, serde_json::Value>,
) -> std::result::Result<String, DocumentError> {
    toml::to_string(value).map_err(|source| DocumentError::RenderFailed {
        reason: source.to_string(),
    })
}

/// Render one map to pretty JSON text.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] for serializer failures.
fn render_json(
    value: &BTreeMap<String, serde_json::Value>,
) -> std::result::Result<String, DocumentError> {
    serde_json::to_string_pretty(value).map_err(|source| DocumentError::RenderFailed {
        reason: source.to_string(),
    })
}

/// Render one map to YAML text.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] for serializer failures.
fn render_yaml(
    value: &BTreeMap<String, serde_json::Value>,
) -> std::result::Result<String, DocumentError> {
    noyalib::to_string(value).map_err(|source| DocumentError::RenderFailed {
        reason: source.to_string(),
    })
}

/// Render rc data to shell text with trailing newline.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn render_rc(data: &RcData) -> std::result::Result<String, DocumentError> {
    let profile = section_lines(&data.profile)?;
    let config = section_lines(&data.config)?;
    let finals = section_lines(&data.r#final)?;
    let mut blocks: Vec<String> = Vec::new();
    if !profile.is_empty() {
        blocks.push(profile.join("\n"));
    }
    if !data.config.is_empty() || !data.r#final.is_empty() {
        blocks.push(GUARD.to_string());
    }
    if !config.is_empty() {
        blocks.push(config.join("\n"));
    }
    if !finals.is_empty() {
        blocks.push(finals.join("\n"));
    }
    if blocks.is_empty() {
        return Ok(String::new());
    }
    let mut out = blocks.join("\n\n");
    out.push('\n');
    Ok(out)
}

/// Collect one section lines in declaration order.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn section_lines(entries: &[RcEntry]) -> std::result::Result<Vec<String>, DocumentError> {
    let mut out = Vec::new();
    for entry in entries {
        out.extend(render_entry(entry)?);
    }
    Ok(out)
}

/// Render one rc entry as shell lines.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn render_entry(entry: &RcEntry) -> std::result::Result<Vec<String>, DocumentError> {
    let line = match entry {
        RcEntry::Env { name, value, .. } => {
            let exported = escape_argv(std::slice::from_ref(value));
            format!("export {name}={exported}")
        }
        RcEntry::Path { name, dir, .. } => {
            let expanded = expand_field(dir)?;
            let placed = quote(&expanded);
            format!("export {name}={placed}:\"${{{name}}}\"")
        }
        RcEntry::Alias {
            name, expansion, ..
        } => {
            let expanded = escape_argv(std::slice::from_ref(expansion));
            format!("alias {name}={expanded}")
        }
        RcEntry::Eval { argv, .. } => format!("eval \"$({})\"", escape_args(argv)?),
        RcEntry::Cmd { argv, .. } => escape_args(argv)?,
        RcEntry::Source { path, .. } => {
            let expanded = expand_field(path)?;
            format!("source {}", quote(&expanded))
        }
    };
    Ok(match entry.when() {
        None => vec![line],
        Some(guard) => vec![
            format!("if {}; then", render_guard(guard)?),
            format!("  {line}"),
            "fi".to_string(),
        ],
    })
}

/// Render one condition as a Bourne test string.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn render_guard(guard: &Condition) -> std::result::Result<String, DocumentError> {
    match guard {
        Condition::EnvEq { key, value } => {
            let rhs = escape_argv(std::slice::from_ref(value));
            Ok(format!("[ \"${{{key}}}\" = {rhs} ]"))
        }
        Condition::EnvSet { key } => Ok(format!("[ -n \"${{{key}}}\" ]")),
        Condition::InPath { name } => {
            let binary = escape_argv(std::slice::from_ref(name));
            Ok(format!("command -v {binary} >/dev/null 2>&1"))
        }
        Condition::Exists { route: found } => {
            let expanded = expand_field(found)?;
            let candidate = escape_argv(std::slice::from_ref(&expanded));
            Ok(format!("[ -e {candidate} ]"))
        }
        // No shell form expresses Changed; it renders false.
        Condition::Changed { .. } => Ok("false".to_string()),
        Condition::All(items) if items.is_empty() => Ok("true".to_string()),
        Condition::Any(items) if items.is_empty() => Ok("false".to_string()),
        Condition::All(items) => join_guards(items, "&&"),
        Condition::Any(items) => join_guards(items, "||"),
        Condition::Not(inner) => match inner.as_ref() {
            Condition::All(_) | Condition::Any(_) | Condition::Not(_) => {
                Ok(format!("! ( {} )", render_guard(inner)?))
            }
            _ => Ok(format!("! {}", render_guard(inner)?)),
        },
    }
}

/// Join nested guards under one operator.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn join_guards(items: &[Condition], op: &str) -> std::result::Result<String, DocumentError> {
    let mut rendered = Vec::with_capacity(items.len());
    for item in items {
        rendered.push(group_guard(item, op)?);
    }
    Ok(rendered.join(&format!(" {op} ")))
}

/// Group one nested guard under an operator.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn group_guard(item: &Condition, parent: &str) -> std::result::Result<String, DocumentError> {
    let rendered = render_guard(item)?;
    Ok(match item {
        Condition::All(_) if parent != "&&" => format!("( {rendered} )"),
        Condition::Any(_) if parent != "||" => format!("( {rendered} )"),
        _ => rendered,
    })
}

/// Render argv as one shell line with Bourne quoting.
fn escape_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote(arg).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render args as one shell line with Bourne quoting.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn escape_args(args: &[Arg]) -> std::result::Result<String, DocumentError> {
    let mut rendered = Vec::with_capacity(args.len());
    for arg in args {
        rendered.push(escape_arg(arg)?);
    }
    Ok(rendered.join(" "))
}

/// Render one arg with Bourne quoting.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn escape_arg(arg: &Arg) -> std::result::Result<String, DocumentError> {
    match arg {
        Arg::Text(text) => Ok(quote(text).into_owned()),
        Arg::Route(found) => {
            let expanded = expand_field(found)?;
            Ok(quote(&expanded).into_owned())
        }
    }
}

/// Expand one route field to its host path.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn expand_field(found: &Route) -> std::result::Result<String, DocumentError> {
    found
        .expand()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| match error.root_cause() {
            ErrorCause::DocumentError(leaf) => leaf.clone(),
            _ => unreachable!("route expansion fails parsing alone"),
        })
}
