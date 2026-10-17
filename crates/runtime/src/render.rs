//! Render
//! Document payloads to on-disk bytes.

use confit_model::arg::{Arg, quote};
use confit_model::condition::Condition;
use confit_model::document::{Data, RcData, RcEntry, StructuredFormat};
use confit_model::routes::Route;

use crate::error::{RefusalShape, Result, RuntimeError};
use crate::manifest::Document;

/// Interactivity guard shared by every shell file.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

/// Render one document to exact on-disk bytes.
///
/// # Errors
///
/// - [`RuntimeError::Refusal`] for opaque with tree payloads.
/// - [`RuntimeError::Render`] for serializer failures.
pub fn render_document(document: &Document) -> Result<Vec<u8>> {
    match &document.data {
        Data::Structured { format, data } => match format {
            StructuredFormat::Toml => Ok(render_toml(&document.destination, data)?.into_bytes()),
            StructuredFormat::Json => Ok(render_json(&document.destination, data)?.into_bytes()),
            StructuredFormat::Yaml => Ok(render_yaml(&document.destination, data)?.into_bytes()),
        },
        Data::Text { content, .. } => Ok(content.as_bytes().to_vec()),
        #[cfg(unix)]
        Data::Link { target } => Ok(target.display().as_bytes().to_vec()),
        Data::Rc(data) => Ok(render_rc(&document.destination, data)?.into_bytes()),
        Data::Opaque { handle: blob, .. } => {
            log::error!(
                "opaque '{}' uses blob '{}'",
                document.destination.display(),
                blob.sha256().hex()
            );
            Err(RuntimeError::Refusal {
                route: document.destination.clone(),
                shape: RefusalShape::Opaque,
            })
        }
        Data::Tree { .. } => Err(RuntimeError::Refusal {
            route: document.destination.clone(),
            shape: RefusalShape::Tree,
        }),
    }
}

/// Render a value to TOML text.
fn render_toml(route: &confit_model::routes::Route, value: &serde_json::Value) -> Result<String> {
    toml::to_string(value).map_err(|source| {
        let reason = source.to_string();
        log::error!("render '{}' as toml failed: {reason}", route.display());
        RuntimeError::Render {
            route: route.clone(),
            format: StructuredFormat::Toml,
            reason,
        }
    })
}

/// Render a value to pretty JSON text.
fn render_json(route: &confit_model::routes::Route, value: &serde_json::Value) -> Result<String> {
    serde_json::to_string_pretty(value).map_err(|source| {
        let reason = source.to_string();
        log::error!("render '{}' as json failed: {reason}", route.display());
        RuntimeError::Render {
            route: route.clone(),
            format: StructuredFormat::Json,
            reason,
        }
    })
}

/// Render a value to YAML text.
fn render_yaml(route: &confit_model::routes::Route, value: &serde_json::Value) -> Result<String> {
    noyalib::to_string(value).map_err(|source| {
        let reason = source.to_string();
        log::error!("render '{}' as yaml failed: {reason}", route.display());
        RuntimeError::Render {
            route: route.clone(),
            format: StructuredFormat::Yaml,
            reason,
        }
    })
}

/// Render rc data to shell text with trailing newline.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn render_rc(route: &Route, data: &RcData) -> Result<String> {
    let profile = section_lines(route, &data.profile)?;
    let config = section_lines(route, &data.config)?;
    let finals = section_lines(route, &data.final_entries)?;
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
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn section_lines(route: &Route, entries: &[RcEntry]) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in entries {
        out.extend(render_entry(route, entry)?);
    }
    Ok(out)
}

/// Render one rc entry as shell lines.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn render_entry(route: &Route, entry: &RcEntry) -> Result<Vec<String>> {
    let line = match entry {
        RcEntry::Env { name, value, .. } => {
            let exported = escape_argv(std::slice::from_ref(value));
            format!("export {name}={exported}")
        }
        RcEntry::Path { name, dir, .. } => {
            let expanded = expand_field(route, dir)?;
            let placed = quote(&expanded);
            format!("export {name}={placed}:\"${{{name}}}\"")
        }
        RcEntry::Alias {
            name, expansion, ..
        } => {
            let expanded = escape_argv(std::slice::from_ref(expansion));
            format!("alias {name}={expanded}")
        }
        RcEntry::Eval { argv, .. } => format!("eval \"$({})\"", escape_args(route, argv)?),
        RcEntry::Cmd { argv, .. } => escape_args(route, argv)?,
        RcEntry::Source { path, .. } => {
            let expanded = expand_field(route, path)?;
            format!("source {}", quote(&expanded))
        }
    };
    Ok(match entry.when() {
        None => vec![line],
        Some(guard) => vec![
            format!("if {}; then", render_guard(route, guard)?),
            format!("  {line}"),
            "fi".to_string(),
        ],
    })
}

/// Render one condition as a Bourne test string.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn render_guard(route: &Route, guard: &Condition) -> Result<String> {
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
            let expanded = expand_field(route, found)?;
            let candidate = escape_argv(std::slice::from_ref(&expanded));
            Ok(format!("[ -e {candidate} ]"))
        }
        // No shell form expresses Changed; it renders false.
        Condition::Changed { .. } => Ok("false".to_string()),
        Condition::All(items) if items.is_empty() => Ok("true".to_string()),
        Condition::Any(items) if items.is_empty() => Ok("false".to_string()),
        Condition::All(items) => join_guards(route, items, "&&"),
        Condition::Any(items) => join_guards(route, items, "||"),
        Condition::Not(inner) => match inner.as_ref() {
            Condition::All(_) | Condition::Any(_) | Condition::Not(_) => {
                Ok(format!("! ( {} )", render_guard(route, inner)?))
            }
            _ => Ok(format!("! {}", render_guard(route, inner)?)),
        },
    }
}

/// Join nested guards under one operator.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn join_guards(route: &Route, items: &[Condition], op: &str) -> Result<String> {
    let mut rendered = Vec::with_capacity(items.len());
    for item in items {
        rendered.push(group_guard(route, item, op)?);
    }
    Ok(rendered.join(&format!(" {op} ")))
}

/// Group one nested guard under an operator.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn group_guard(route: &Route, item: &Condition, parent: &str) -> Result<String> {
    let rendered = render_guard(route, item)?;
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
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn escape_args(route: &Route, args: &[Arg]) -> Result<String> {
    let mut rendered = Vec::with_capacity(args.len());
    for arg in args {
        rendered.push(escape_arg(route, arg)?);
    }
    Ok(rendered.join(" "))
}

/// Render one arg with Bourne quoting.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn escape_arg(route: &Route, arg: &Arg) -> Result<String> {
    match arg {
        Arg::Text(text) => Ok(quote(text).into_owned()),
        Arg::Route(found) => {
            let expanded = expand_field(route, found)?;
            Ok(quote(&expanded).into_owned())
        }
    }
}

/// Expand one route field against its document route for errors.
///
/// # Errors
///
/// - [`RuntimeError::Expand`] for unresolvable base folders.
fn expand_field(route: &Route, found: &Route) -> Result<String> {
    found
        .expand()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|source| RuntimeError::Expand {
            route: route.clone(),
            source,
        })
}
