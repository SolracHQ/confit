//! Render
//!
//! Document payloads to on-disk bytes.

use std::path::PathBuf;

use crate::arg::{Arg, quote};
use crate::condition::Condition;
use crate::document::{
    ManifestData, ManifestDocument, RcData, RcEntry, RcOp, StructuredFormat, Table,
};
use crate::error::{Error, Result};
use crate::handles::Route;

/// Interactivity guard shared by every shell file.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

impl ManifestDocument {
    /// Renders one document to exact on-disk bytes.
    ///
    /// # Errors
    ///
    /// Opaque and tree payloads fail as plan errors; their
    /// bytes ride the blob store.
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::path::PathBuf;
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::handles::{Route, RouteBase};
    ///
    /// let document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.render(&|route| PathBuf::from(route.display())), Ok(bytes) if bytes == b"hi".to_vec()));
    /// ```
    pub fn render(&self, resolve: &dyn Fn(&Route) -> PathBuf) -> Result<Vec<u8>> {
        render_inline_bytes(&self.data, resolve)
    }
}

/// Renders inline payload bytes without blob access.
///
/// # Errors
///
/// Opaque, tree, and serializer failures fail
/// as plan errors.
pub(crate) fn render_inline_bytes(
    data: &ManifestData,
    resolve: &dyn Fn(&Route) -> PathBuf,
) -> Result<Vec<u8>> {
    match data {
        ManifestData::Structured { format, data } => match format {
            StructuredFormat::Toml => Ok(render_toml(data)?.into_bytes()),
            StructuredFormat::Json => Ok(render_json(data)?.into_bytes()),
            StructuredFormat::Yaml => Ok(render_yaml(data)?.into_bytes()),
        },
        ManifestData::Text { content, .. } => Ok(content.as_bytes().to_vec()),
        ManifestData::Link { target } => Ok(target.as_bytes().to_vec()),
        ManifestData::Rc(data) => Ok(render_rc(data, resolve).into_bytes()),
        ManifestData::Opaque { blob, .. } => Err(Error::Plan(format!(
            "render opaque '{}': blob bytes ride the blob store",
            blob.sha()
        ))),
        ManifestData::Tree { .. } => Err(Error::Plan(
            "render tree: tree documents hold member bytes".to_string(),
        )),
    }
}

/// Renders a table to TOML text.
fn render_toml(table: &Table) -> Result<String> {
    toml::to_string(table).map_err(|error| Error::Plan(format!("render toml: {error}")))
}

/// Renders a table to pretty JSON text.
fn render_json(table: &Table) -> Result<String> {
    serde_json::to_string_pretty(table)
        .map_err(|error| Error::Plan(format!("render json: {error}")))
}

/// Renders a table to YAML text.
fn render_yaml(table: &Table) -> Result<String> {
    noyalib::to_string(table).map_err(|error| Error::Plan(format!("render yaml: {error}")))
}

/// Renders rc data to shell text with trailing newline.
fn render_rc(data: &RcData, resolve: &dyn Fn(&Route) -> PathBuf) -> String {
    let profile = section_lines(&data.profile, resolve);
    let config = section_lines(&data.config, resolve);
    let finals = section_lines(&data.final_entries, resolve);
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

/// Collects one section lines in declaration order.
fn section_lines(entries: &[RcEntry], resolve: &dyn Fn(&Route) -> PathBuf) -> Vec<String> {
    entries
        .iter()
        .flat_map(|entry| render_entry(entry, resolve))
        .collect()
}

/// Renders one rc entry as shell lines.
fn render_entry(entry: &RcEntry, resolve: &dyn Fn(&Route) -> PathBuf) -> Vec<String> {
    let line = match &entry.op {
        RcOp::Env { name, value } => {
            let exported = escape_argv(std::slice::from_ref(value));
            format!("export {name}={exported}")
        }
        RcOp::Path { name, dir, .. } => {
            let expanded = resolve(dir).to_string_lossy().into_owned();
            let placed = quote(&expanded);
            format!("export {name}={placed}:\"${{{name}}}\"")
        }
        RcOp::Alias { name, expansion } => {
            let expanded = escape_argv(std::slice::from_ref(expansion));
            format!("alias {name}={expanded}")
        }
        RcOp::Eval { argv, .. } => format!("eval \"$({})\"", escape_args(argv, resolve)),
        RcOp::Cmd { argv, .. } => escape_args(argv, resolve),
        RcOp::Source { path, .. } => {
            let expanded = resolve(path).to_string_lossy().into_owned();
            format!("source {}", quote(&expanded))
        }
    };
    match entry.when.as_ref() {
        None => vec![line],
        Some(guard) => vec![
            format!("if {}; then", render_guard(guard)),
            format!("  {line}"),
            "fi".to_string(),
        ],
    }
}

/// Renders one condition as a Bourne test string.
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
        Condition::Exists { path } => {
            let candidate = escape_argv(std::slice::from_ref(path));
            format!("[ -e {candidate} ]")
        }
        // Changed never reaches shell guards: rc guards holding it fail
        // as plan errors at build time. Render false so entries stay
        // quiet if the invariant ever breaks.
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

/// Joins nested guards under one operator.
fn join_guards(items: &[Condition], op: &str) -> String {
    items
        .iter()
        .map(|item| group_guard(item, op))
        .collect::<Vec<_>>()
        .join(&format!(" {op} "))
}

/// Groups one nested guard for joining under an operator.
fn group_guard(item: &Condition, parent: &str) -> String {
    let rendered = render_guard(item);
    match item {
        Condition::All(_) if parent != "&&" => format!("( {rendered} )"),
        Condition::Any(_) if parent != "||" => format!("( {rendered} )"),
        _ => rendered,
    }
}

/// Renders argv as one shell line with Bourne quoting.
fn escape_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|arg| quote(arg).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders slots as one shell line with Bourne quoting.
fn escape_args(slots: &[Arg], resolve: &dyn Fn(&Route) -> PathBuf) -> String {
    slots
        .iter()
        .map(|slot| escape_slot(slot, resolve))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renders one slot with Bourne quoting.
fn escape_slot(slot: &Arg, resolve: &dyn Fn(&Route) -> PathBuf) -> String {
    match slot {
        Arg::Text(text) => quote(text).into_owned(),
        Arg::Route(route) => {
            let expanded = resolve(route).to_string_lossy().into_owned();
            quote(&expanded).into_owned()
        }
    }
}
