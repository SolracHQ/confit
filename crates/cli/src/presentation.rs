//! Presentation
//!
//! Terminal summary plus plan payload text.

use std::borrow::Cow;
use std::collections::BTreeSet;

use confit_core::document::{Document, DocumentData, RcOp};
use confit_core::ids::DocPath;
use confit_core::plan::{BuiltPlan, DocumentStatus, PlanSummary, State, Warning, document_status};

/// Yellow style for changed lines.
const UPDATE_STYLE: &str = "\x1b[33m";
/// Green style for added lines.
const ADD_STYLE: &str = "\x1b[32m";
/// Red style for removed lines.
const REMOVE_STYLE: &str = "\x1b[31m";
/// Bold style for headers plus the closing counts.
const HEADER_STYLE: &str = "\x1b[1m";
/// Style reset suffix.
const RESET: &str = "\x1b[0m";

/// Line sigil selecting paint color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sigil {
    /// Addition, painted green.
    Add,
    /// Change, painted yellow.
    Update,
    /// Removal, painted red.
    Remove,
    /// Header, painted bold.
    Header,
}

/// Reports whether color output applies.
///
/// Piped output stays plain. `NO_COLOR` disables color.
///
/// # Returns
///
/// True while stderr runs as a terminal without `NO_COLOR`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::color_on;
///
/// assert!(matches!(color_on(), true | false));
/// ```
pub fn color_on() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::IsTerminal::is_terminal(&std::io::stderr())
}

/// Paints one line in the sigil style.
///
/// # Arguments
///
/// * `sigil` - the style selecting the line color.
/// * `text` - the line body under paint.
///
/// # Returns
///
/// The painted line ready for terminal display.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::paint_add;
///
/// assert!(matches!(paint_add("hi").is_empty(), false));
/// ```
pub fn paint_add(text: &str) -> String {
    paint(Sigil::Add, text)
}

/// Paints one line in the sigil style.
///
/// # Arguments
///
/// * `sigil` - the style selecting the line color.
/// * `text` - the line body under paint.
///
/// # Returns
///
/// The painted line ready for terminal display.
fn paint(sigil: Sigil, text: &str) -> String {
    if !color_on() {
        return text.to_string();
    }
    let style = match sigil {
        Sigil::Add => ADD_STYLE,
        Sigil::Update => UPDATE_STYLE,
        Sigil::Remove => REMOVE_STYLE,
        Sigil::Header => HEADER_STYLE,
    };
    format!("{style}{text}{RESET}")
}

/// Renders warnings verbatim in outcome order.
///
/// # Arguments
///
/// * `warnings` - the warnings under display.
///
/// # Returns
///
/// One stderr line per warning through `Warning::line`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::warning_lines;
///
/// assert!(matches!(warning_lines(&[]).len(), 0));
/// ```
pub fn warning_lines(warnings: &[Warning]) -> Vec<String> {
    warnings.iter().map(Warning::line).collect()
}

/// Renders the closing counts line.
///
/// # Arguments
///
/// * `summary` - the lifecycle counts under display.
///
/// # Returns
///
/// The `Plan: {create} to add` closing line.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::summary_line;
/// use confit_core::plan::PlanSummary;
///
/// let line = summary_line(&PlanSummary { create: 2, update: 0, delete: 0 });
/// assert!(matches!(line.as_str(), "Plan: 2 to add, 0 to change, 0 to destroy."));
/// ```
pub fn summary_line(summary: &PlanSummary) -> String {
    format!(
        "Plan: {} to add, {} to change, {} to destroy.",
        summary.create, summary.update, summary.delete
    )
}

/// Reads the display label for one document.
///
/// Structured documents show the format name. Other kinds show
/// the kind name.
///
/// # Arguments
///
/// * `document` - the document naming the header.
///
/// # Returns
///
/// The label following the path in the header line.
fn doc_label(document: &Document) -> Cow<'_, str> {
    match &document.data {
        DocumentData::Structured { format, .. } => Cow::Borrowed(format.name()),
        _ => Cow::Borrowed(document.data.kind().name()),
    }
}

/// Reads the header line for one document.
///
/// # Arguments
///
/// * `document` - the document naming the header.
///
/// # Returns
///
/// The `{path}: {label}` header line.
fn header_line(document: &Document) -> String {
    format!("{}: {}", document.path.as_str(), doc_label(document))
}

/// Renders one scalar leaf value as display text.
fn leaf_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Number(_) | serde_json::Value::Bool(_) => value.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
        }
    }
}

/// Collects flattened leaf lines for one table.
fn table_leaves(
    table: &confit_core::document::Table,
    prefix: &str,
    out: &mut Vec<(String, String)>,
) {
    for (key, value) in table {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        collect_value(&full, value, out);
    }
}

/// Collects leaf lines for one value under its dotted key.
fn collect_value(key: &str, value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    match value {
        serde_json::Value::Object(map) => {
            for (inner, item) in map {
                collect_value(&format!("{key}.{inner}"), item, out);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                collect_value(&format!("{key}[{index}]"), item, out);
            }
        }
        _ => out.push((key.to_string(), leaf_text(value))),
    }
}

/// Renders one exec payload as shell text.
fn init_text(op: &RcOp) -> String {
    match op {
        RcOp::Eval { argv, .. } => format!("eval \"$({})\"", argv.join(" ")),
        RcOp::Cmd { argv, .. } => argv.join(" "),
        RcOp::Source { path, .. } => format!("source {path}"),
        RcOp::Env { name, value } => format!("profile {name} = {value}"),
        RcOp::Path { name, dir, .. } => format!("profile {name} = {dir}"),
        RcOp::Alias { name, expansion } => format!("alias {name} = {expansion}"),
    }
}

/// Renders one named rc entry as display text.
fn named_text(op: &RcOp) -> String {
    match op {
        RcOp::Env { name, value } => format!("profile {name} = {value}"),
        RcOp::Path { name, dir, .. } => format!("profile {name} = {dir}"),
        RcOp::Alias { name, expansion } => format!("alias {name} = {expansion}"),
        _ => init_text(op),
    }
}

/// Collects plain entry bodies for one document.
fn entry_bodies(document: &Document) -> Vec<String> {
    match &document.data {
        DocumentData::Structured { data, .. } => {
            let mut leaves = Vec::new();
            table_leaves(data, "", &mut leaves);
            leaves
                .into_iter()
                .map(|(key, value)| format!("{key} = {value}"))
                .collect()
        }
        DocumentData::Text { content } => {
            if content.is_empty() {
                Vec::new()
            } else {
                content.split('\n').map(str::to_string).collect()
            }
        }
        DocumentData::Link { target } => vec![target.clone()],
        DocumentData::Rc(rc) => {
            let mut out = Vec::new();
            for entry in rc.profile.iter().chain(rc.config.iter()) {
                match &entry.op {
                    RcOp::Env { .. } | RcOp::Path { .. } | RcOp::Alias { .. } => {
                        out.push(named_text(&entry.op));
                    }
                    op => out.push(init_text(op)),
                }
            }
            for (index, entry) in rc.final_entries.iter().enumerate() {
                match &entry.op {
                    RcOp::Env { .. } | RcOp::Path { .. } | RcOp::Alias { .. } => {
                        out.push(named_text(&entry.op));
                    }
                    op => out.push(format!("init[{index}] = {}", init_text(op))),
                }
            }
            out
        }
    }
}

/// Renders entry lines for one document under its status.
///
/// Create entries carry `+`, update entries carry `~`,
/// unchanged documents carry zero lines.
///
/// # Arguments
///
/// * `document` - the document backing the entries.
/// * `status` - the lifecycle status selecting the sigil.
///
/// # Returns
///
/// Plain entry lines with two-space indent.
fn entry_lines(document: &Document, status: DocumentStatus) -> Vec<String> {
    let mark = match status {
        DocumentStatus::Create => '+',
        DocumentStatus::Update => '~',
        DocumentStatus::Unchanged => return Vec::new(),
    };
    entry_bodies(document)
        .into_iter()
        .map(|body| format!("  {mark} {body}"))
        .collect()
}

/// Splits a state key into kind plus path halves.
fn split_key(key: &str) -> (&str, &str) {
    match key.find(':') {
        Some(index) => (&key[..index], &key[index + 1..]),
        None => ("", key),
    }
}

/// Renders delete headers for previous keys missing from the plan.
///
/// # Arguments
///
/// * `built` - the built plan holding desired documents.
/// * `previous` - the last apply record.
///
/// # Returns
///
/// One `{path}: {kind}` header per removed key in sorted order.
fn delete_headers(built: &BuiltPlan, previous: &State) -> Vec<String> {
    let seen: BTreeSet<String> = built
        .plan
        .documents
        .iter()
        .map(|document| document.key())
        .collect();
    previous
        .documents
        .keys()
        .filter(|key| !seen.contains(*key))
        .map(|key| {
            let (kind, path) = split_key(key);
            DocPath::new(path);
            format!("{path}: {kind}")
        })
        .collect()
}

/// Renders the full stderr summary with color.
///
/// Warnings lead verbatim, so disk drift notes precede the plan
/// half. Per-document headers plus entry lines follow. The counts
/// line closes the text.
///
/// # Arguments
///
/// * `built` - the built plan with counts plus warnings.
/// * `previous` - the last apply record for lifecycle marks.
///
/// # Returns
///
/// The stderr summary text.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::render;
/// use confit_core::ids::{DocPath, ReadOutcome};
/// use confit_core::plan::{State, build};
/// use confit_core::document::{Document, DocumentData};
///
/// let document = Document::new(
///     DocPath::new("note"),
///     DocumentData::Text { content: "hi".into() },
/// );
/// assert!(matches!(document.data, DocumentData::Text { .. }));
/// ```
pub fn render(built: &BuiltPlan, previous: &State) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in warning_lines(&built.warnings) {
        lines.push(line);
    }
    for document in &built.plan.documents {
        lines.push(paint(Sigil::Header, &header_line(document)));
        let status = document_status(document, previous);
        let sigil = match status {
            DocumentStatus::Create => Sigil::Add,
            DocumentStatus::Update => Sigil::Update,
            DocumentStatus::Unchanged => Sigil::Header,
        };
        for entry in entry_lines(document, status) {
            if matches!(status, DocumentStatus::Unchanged) {
                lines.push(entry);
            } else {
                lines.push(paint(sigil, &entry));
            }
        }
    }
    for header in delete_headers(built, previous) {
        lines.push(paint(Sigil::Remove, &header));
    }
    lines.push(paint(Sigil::Header, &summary_line(&built.summary)));
    lines.join("\n")
}

/// Renders the plan payload as pretty JSON.
///
/// # Arguments
///
/// * `built` - the built plan holding the payload.
///
/// # Returns
///
/// The pretty plan JSON for file or stdout use.
///
/// # Errors
///
/// Serializer failures fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::payload;
/// use confit_core::ids::ReadOutcome;
/// use confit_core::plan::{State, build};
///
/// let built = build(Vec::new(), &State::empty(), &|_| ReadOutcome::Absent);
/// assert!(matches!(built, Ok(ref built) if matches!(payload(built), Ok(_))));
/// ```
pub fn payload(built: &BuiltPlan) -> confit_core::error::Result<String> {
    serde_json::to_string_pretty(&built.plan)
        .map_err(|error| confit_core::error::Error::Plan(format!("render plan: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_line_counts_add_change_destroy() {
        let line = summary_line(&PlanSummary {
            create: 2,
            update: 1,
            delete: 3,
        });
        assert_eq!(line, "Plan: 2 to add, 1 to change, 3 to destroy.");
    }

    #[test]
    fn render_lists_create_entries_plus_closing_counts() {
        use confit_core::document::DocumentData;
        use confit_core::ids::ReadOutcome;

        let document = Document::new(
            DocPath::new("~/.bashrc"),
            DocumentData::Text {
                content: "hi".to_string(),
            },
        );
        let built = match confit_core::plan::build(vec![document], &State::empty(), &|_| {
            ReadOutcome::Absent
        }) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let text = render(&built, &State::empty());
        assert!(text.contains("~/.bashrc: text"));
        assert!(text.contains("  + hi"));
        assert!(text.contains("Plan: 1 to add, 0 to change, 0 to destroy."));
    }
}
