//! Document payloads to on-disk bytes.

use std::collections::BTreeMap;

use crate::arg::{Arg, quote};
use crate::condition::Condition;
use crate::document::{RcData, RcEntry};
use crate::routes::Route;
use confit_core::error::{DocumentError, ErrorCause};

/// The guard that every shell file shares for an interactive shell.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

/// Encodes one map as TOML text.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] when the serializer fails.
pub(crate) fn render_toml(
    value: &BTreeMap<String, serde_json::Value>,
) -> std::result::Result<String, DocumentError> {
    toml::to_string(value).map_err(|_| DocumentError::RenderFailed)
}

/// Encodes one map as pretty JSON text.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] when the serializer fails.
pub(crate) fn render_json(
    value: &BTreeMap<String, serde_json::Value>,
) -> std::result::Result<String, DocumentError> {
    serde_json::to_string_pretty(value).map_err(|_| DocumentError::RenderFailed)
}

/// Encodes one map as YAML text.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] when the serializer fails.
pub(crate) fn render_yaml(
    value: &BTreeMap<String, serde_json::Value>,
) -> std::result::Result<String, DocumentError> {
    noyalib::to_string(value).map_err(|_| DocumentError::RenderFailed)
}

/// Encodes rc data as shell text with a trailing newline.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
pub(crate) fn render_rc(data: &RcData) -> std::result::Result<String, DocumentError> {
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

/// Collects the lines of one section in declaration order.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn section_lines(entries: &[RcEntry]) -> std::result::Result<Vec<String>, DocumentError> {
    let mut out = Vec::new();
    for entry in entries {
        out.extend(render_entry(entry)?);
    }
    Ok(out)
}

/// Encodes one rc entry as shell lines.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
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

/// Encodes one condition as a Bourne test string.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
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

/// Joins the nested guards with one operator.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn join_guards(items: &[Condition], op: &str) -> std::result::Result<String, DocumentError> {
    let mut rendered = Vec::with_capacity(items.len());
    for item in items {
        rendered.push(group_guard(item, op)?);
    }
    Ok(rendered.join(&format!(" {op} ")))
}

/// Groups one nested guard with its operator.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn group_guard(item: &Condition, parent: &str) -> std::result::Result<String, DocumentError> {
    let rendered = render_guard(item)?;
    Ok(match item {
        Condition::All(_) if parent != "&&" => format!("( {rendered} )"),
        Condition::Any(_) if parent != "||" => format!("( {rendered} )"),
        _ => rendered,
    })
}

/// Encodes the argv as one shell line with Bourne quoting.
fn escape_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote(arg).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Encodes the args as one shell line with Bourne quoting.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn escape_args(args: &[Arg]) -> std::result::Result<String, DocumentError> {
    let mut rendered = Vec::with_capacity(args.len());
    for arg in args {
        rendered.push(escape_arg(arg)?);
    }
    Ok(rendered.join(" "))
}

/// Encodes one arg with Bourne quoting.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn escape_arg(arg: &Arg) -> std::result::Result<String, DocumentError> {
    match arg {
        Arg::Text(text) => Ok(quote(text).into_owned()),
        Arg::Route(found) => {
            let expanded = expand_field(found)?;
            Ok(quote(&expanded).into_owned())
        }
    }
}

/// Expands one route field to the path on the host.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn expand_field(found: &Route) -> std::result::Result<String, DocumentError> {
    found
        .expand()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|contextual| match contextual.cause() {
            ErrorCause::DocumentError(leaf) => leaf.clone(),
            _ => unreachable!("expected a parse fault from route expansion"),
        })
}
