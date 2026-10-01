//! Render
//!
//! Document payloads to on-disk bytes.

use confit_model::arg::{Arg, quote};
use confit_model::condition::Condition;
use confit_model::document::{Data, Document, RcData, RcEntry, RcOp, StructuredFormat, Table};
use confit_model::error::{Error, Result};

/// Interactivity guard shared by every shell file.
const GUARD: &str = "case $- in\n*i*) ;;\n*) return ;;\nesac";

/// Renders one document to exact on-disk bytes.
///
/// Routes expand against host folders, so shell
/// payloads carry resolved paths.
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
/// use confit_model::document::{Data, Document};
/// use confit_model::routes::{Route, RouteBase};
/// use confit_store::StoreRoots;
/// use confit_runtime::Applier;
///
/// let document = Document::new(
///     Route::new(RouteBase::Home, "note").unwrap(),
///     Data::Text { content: "hi".into(), mode: None, unmanaged: false},
/// );
/// let applier = Applier::host(StoreRoots::default());
/// assert!(matches!(applier.render_document(&document), Ok(bytes) if bytes == b"hi".to_vec()));
/// ```
impl crate::Applier {
    /// Renders one document to exact on-disk bytes.
    ///
    /// Routes expand against host folders, so shell
    /// payloads carry resolved paths.
    ///
    /// # Errors
    ///
    /// Opaque and tree payloads fail as plan errors; their
    /// bytes ride the blob store.
    /// Serializer failures fail as plan errors.
    pub fn render_document(&self, document: &Document) -> Result<Vec<u8>> {
        match &document.data {
            Data::Structured { format, data } => match format {
                StructuredFormat::Toml => Ok(render_toml(data)?.into_bytes()),
                StructuredFormat::Json => Ok(render_json(data)?.into_bytes()),
                StructuredFormat::Yaml => Ok(render_yaml(data)?.into_bytes()),
            },
            Data::Text { content, .. } => Ok(content.as_bytes().to_vec()),
            Data::Link { target } => Ok(target.as_bytes().to_vec()),
            Data::Rc(data) => Ok(render_rc(data).into_bytes()),
            Data::Opaque { blob, .. } => Err(Error::Plan(format!(
                "render opaque '{}': blob bytes ride the blob store",
                blob.sha()
            ))),
            Data::Tree { .. } => Err(Error::Plan(
                "render tree: tree documents hold member bytes".to_string(),
            )),
        }
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

/// Collects one section lines in declaration order.
fn section_lines(entries: &[RcEntry]) -> Vec<String> {
    entries.iter().flat_map(render_entry).collect()
}

/// Renders one rc entry as shell lines.
fn render_entry(entry: &RcEntry) -> Vec<String> {
    let line = match &entry.op {
        RcOp::Env { name, value } => {
            let exported = escape_argv(std::slice::from_ref(value));
            format!("export {name}={exported}")
        }
        RcOp::Path { name, dir } => {
            let expanded = dir.expand().to_string_lossy().into_owned();
            let placed = quote(&expanded);
            format!("export {name}={placed}:\"${{{name}}}\"")
        }
        RcOp::Alias { name, expansion } => {
            let expanded = escape_argv(std::slice::from_ref(expansion));
            format!("alias {name}={expanded}")
        }
        RcOp::Eval { argv, .. } => format!("eval \"$({})\"", escape_args(argv)),
        RcOp::Cmd { argv, .. } => escape_args(argv),
        RcOp::Source { path, .. } => {
            let expanded = path.expand().to_string_lossy().into_owned();
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
        Condition::Exists { route } => {
            let expanded = route.expand().to_string_lossy().into_owned();
            let candidate = escape_argv(std::slice::from_ref(&expanded));
            format!("[ -e {candidate} ]")
        }
        // Changed holds no shell form and renders false.
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
fn escape_args(slots: &[Arg]) -> String {
    slots.iter().map(escape_slot).collect::<Vec<_>>().join(" ")
}

/// Renders one slot with Bourne quoting.
fn escape_slot(slot: &Arg) -> String {
    match slot {
        Arg::Text(text) => quote(text).into_owned(),
        Arg::Route(route) => {
            let expanded = route.expand().to_string_lossy().into_owned();
            quote(&expanded).into_owned()
        }
    }
}
