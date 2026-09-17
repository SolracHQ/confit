//! Summary
//!
//! Stderr summary over built plans.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use confit_core::document::{Document, DocumentData, DocumentKind, RcOp, Table};
use confit_core::drift::Drift;
use confit_core::plan::opaque_label;
use confit_core::plan::{DocumentStatus, Plan};

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
pub enum Sigil {
    /// Addition, painted green.
    Add,
    /// Change, painted yellow.
    Update,
    /// Removal, painted red.
    Remove,
    /// Header, painted bold.
    Header,
}

/// One terminal painter holding the color decision.
///
/// The tty plus `NO_COLOR` check runs once under construction,
/// never per line. Piped output stays plain.
///
/// # Examples
///
/// ```text
/// use confit_cli::presentation::summary::{Painter, Sigil};
///
/// let painter = Painter::new();
/// assert!(matches!(painter.paint(Sigil::Add, "hi").is_empty(), false));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Painter {
    /// Paints while stderr runs as a terminal without `NO_COLOR`.
    color: bool,
}

impl Painter {
    /// Reads the color decision from the terminal plus the environment.
    ///
    /// # Returns
    ///
    /// The painter for one summary run.
    pub fn new() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none()
                && std::io::IsTerminal::is_terminal(&std::io::stderr()),
        }
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
    pub fn paint(&self, sigil: Sigil, text: &str) -> String {
        if !self.color {
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
}

impl Default for Painter {
    /// Reads the color decision like `new`.
    fn default() -> Self {
        Self::new()
    }
}

/// One stderr summary over a built plan plus its previous plan.
///
/// Drift notes lead verbatim, so plan versus disk edits
/// precede the plan half. Moving documents follow with
/// headers plus entry lines. The counts line closes the text.
///
/// # Examples
///
/// ```text
/// use confit_cli::presentation::summary::Summary;
/// use confit_core::ids::DocPath;
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::plan::Plan;
///
/// let document = Document::new(
///     DocPath::new("note"),
///     DocumentData::Text { content: "hi".into() },
/// );
/// let built = Plan::build(vec![document], Vec::new());
/// let previous = Plan::empty();
/// let summary = match built {
///     Ok(ref built) => Summary { built, previous: &previous, drift: &[] },
///     Err(error) => panic!("plan builds: {error}"),
/// };
/// assert!(matches!(summary.render().len(), _));
/// ```
#[derive(Debug)]
pub struct Summary<'a> {
    /// Holds the built plan under display.
    pub built: &'a Plan,
    /// Holds the previous plan for lifecycle marks.
    pub previous: &'a Plan,
    /// Holds the plan versus disk edits leading the text.
    pub drift: &'a [Drift],
}

impl Summary<'_> {
    /// Renders the full stderr summary with color.
    ///
    /// # Returns
    ///
    /// The stderr summary text.
    pub fn render(&self) -> String {
        let painter = Painter::new();
        let mut lines: Vec<String> = Vec::new();
        for line in Drift::lines(self.drift) {
            lines.push(line);
        }
        for document in &self.built.documents {
            let status = document.status(self.previous);
            if matches!(status, DocumentStatus::Unchanged) {
                continue;
            }
            lines.push(painter.paint(Sigil::Header, &header_line(document)));
            let sigil = match status {
                DocumentStatus::Create => Sigil::Add,
                DocumentStatus::Update => Sigil::Update,
                DocumentStatus::Unchanged => Sigil::Header,
            };
            for entry in self.document_lines(document) {
                lines.push(painter.paint(sigil, &entry));
            }
        }
        for header in self.delete_headers() {
            lines.push(painter.paint(Sigil::Remove, &header));
        }
        lines.push(painter.paint(Sigil::Header, &self.summary_line()));
        lines.join("\n")
    }

    /// Renders the closing counts line.
    ///
    /// # Returns
    ///
    /// The `Plan: {create} to add` closing line.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_cli::presentation::summary::Summary;
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::Plan;
    ///
    /// let first = Document::new(DocPath::new("a"), DocumentData::Text { content: "a".into() });
    /// let second = Document::new(DocPath::new("b"), DocumentData::Text { content: "b".into() });
    /// let built = Plan::build(vec![first, second], Vec::new());
    /// let previous = Plan::empty();
    /// let summary = match built {
    ///     Ok(ref built) => Summary { built, previous: &previous, drift: &[] },
    ///     Err(error) => panic!("plan builds: {error}"),
    /// };
    /// assert!(matches!(summary.summary_line().as_str(), "Plan: 2 to add, 0 to change, 0 to destroy."));
    /// ```
    pub fn summary_line(&self) -> String {
        let summary = self.built.summary(self.previous);
        format!(
            "Plan: {} to add, {} to change, {} to destroy.",
            summary.create, summary.update, summary.delete
        )
    }

    /// Renders entry lines for one document under its own status.
    ///
    /// Status plus the recorded document derive from the report,
    /// so callers pass the document alone.
    fn document_lines(&self, document: &Document) -> Vec<String> {
        match document.status(self.previous) {
            DocumentStatus::Create => entry_bodies(document)
                .into_iter()
                .map(|body| format!("  + {body}"))
                .collect(),
            DocumentStatus::Update => match self.find_recorded(document) {
                Some(old) => update_lines(document, old),
                None => entry_bodies(document)
                    .into_iter()
                    .map(|body| format!("  ~ {body}"))
                    .collect(),
            },
            DocumentStatus::Unchanged => Vec::new(),
        }
    }

    /// Renders delete headers for previous keys missing from the plan.
    fn delete_headers(&self) -> Vec<String> {
        let seen: BTreeSet<String> = self
            .built
            .documents
            .iter()
            .map(|document| document.key())
            .collect();
        self.previous
            .documents
            .iter()
            .filter(|recorded| {
                !seen.contains(&recorded.key()) && !recorded.superseded_by(&self.built.documents)
            })
            .map(|recorded| {
                let key = recorded.key();
                let (kind, path) = split_key(&key);
                match &recorded.data {
                    DocumentData::Tree { members } => {
                        format!("{path}: {kind} ({} files)", members.len())
                    }
                    _ => format!("{path}: {kind}"),
                }
            })
            .collect()
    }

    /// Finds one recorded document by key with opaque fallback.
    fn find_recorded<'a>(&'a self, document: &Document) -> Option<&'a Document> {
        if let Some(found) = self
            .previous
            .documents
            .iter()
            .find(|item| item.key() == document.key())
        {
            return Some(found);
        }
        self.previous.documents.iter().find(|recorded| {
            recorded.path == document.path
                && recorded.key() != document.key()
                && (recorded.is_opaque() || document.is_opaque())
        })
    }
}

/// Reads the display label for one document.
fn doc_label(document: &Document) -> Cow<'_, str> {
    match &document.data {
        DocumentData::Structured { format, .. } => Cow::Borrowed(format.name()),
        _ => Cow::Borrowed(document.data.kind().name()),
    }
}

/// Reads the header line for one document.
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
fn table_leaves(table: &Table, prefix: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (key, value) in table {
        let full = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        out.extend(collect_value(&full, value));
    }
    out
}

/// Collects leaf lines for one value under its dotted key.
fn collect_value(key: &str, value: &serde_json::Value) -> Vec<(String, String)> {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = Vec::new();
            for (inner, item) in map {
                out.extend(collect_value(&format!("{key}.{inner}"), item));
            }
            out
        }
        serde_json::Value::Array(items) => {
            let mut out = Vec::new();
            for (index, item) in items.iter().enumerate() {
                out.extend(collect_value(&format!("{key}[{index}]"), item));
            }
            out
        }
        _ => vec![(key.to_string(), leaf_text(value))],
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
        DocumentData::Structured { data, .. } => table_leaves(data, "")
            .into_iter()
            .map(|(key, value)| format!("{key} = {value}"))
            .collect(),
        DocumentData::Text { content, .. } => {
            if content.is_empty() {
                Vec::new()
            } else {
                content.split('\n').map(str::to_string).collect()
            }
        }
        DocumentData::Link { target } => vec![target.clone()],
        DocumentData::Opaque { content, .. } => vec![format!("opaque ({} bytes)", content.len())],
        DocumentData::Tree { members } => vec![format!("tree ({} files)", members.len())],
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

/// Reports whether either side carries the opaque kind.
fn touches_opaque(first: &Document, second: &Document) -> bool {
    matches!(first.data.kind(), DocumentKind::Opaque)
        || matches!(second.data.kind(), DocumentKind::Opaque)
}

/// Flattens one JSON value into dotted leaf entries.
fn flatten_json(key: &str, value: &serde_json::Value) -> BTreeMap<String, serde_json::Value> {
    let mut out = BTreeMap::new();
    match value {
        serde_json::Value::Object(map) => {
            for (inner, item) in map {
                out.extend(flatten_json(&format!("{key}.{inner}"), item));
            }
        }
        serde_json::Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                out.extend(flatten_json(&format!("{key}[{index}]"), item));
            }
        }
        _ => {
            out.insert(key.to_string(), value.clone());
        }
    }
    out
}

/// Collects update lines with old to new values.
fn update_lines(document: &Document, recorded: &Document) -> Vec<String> {
    match (&document.data, &recorded.data) {
        (
            DocumentData::Structured { data: new, .. },
            DocumentData::Structured { data: old, .. },
        ) => {
            let mut old_flat = BTreeMap::new();
            for (key, value) in old {
                old_flat.extend(flatten_json(key, value));
            }
            let mut new_flat = BTreeMap::new();
            for (key, value) in new {
                new_flat.extend(flatten_json(key, value));
            }
            let mut keys = BTreeSet::new();
            keys.extend(old_flat.keys().cloned());
            keys.extend(new_flat.keys().cloned());
            let mut out = Vec::new();
            let mut sorted: Vec<String> = keys.into_iter().collect();
            sorted.sort();
            for key in sorted {
                let old_value = old_flat
                    .get(&key)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                let new_value = new_flat
                    .get(&key)
                    .cloned()
                    .unwrap_or(serde_json::Value::Null);
                if old_value != new_value {
                    out.push(format!(
                        "  ~ {key} = {} -> {}",
                        leaf_text(&old_value),
                        leaf_text(&new_value)
                    ));
                }
            }
            out
        }
        (DocumentData::Link { target: new }, DocumentData::Link { target: old }) => {
            if old == new {
                Vec::new()
            } else {
                vec![format!("  ~ target = {old} -> {new}")]
            }
        }
        (DocumentData::Opaque { content: new, .. }, DocumentData::Opaque { content: old, .. }) => {
            if old == new {
                Vec::new()
            } else {
                vec![format!(
                    "  ~ content = {} -> {}",
                    opaque_label(old),
                    opaque_label(new)
                )]
            }
        }
        (DocumentData::Tree { members: new }, DocumentData::Tree { members: old }) => {
            let changed = confit_core::document::tree_changed(old, new);
            vec![format!(
                "  ~ tree ({changed} of {} files changed)",
                new.len()
            )]
        }
        _ if touches_opaque(document, recorded) => {
            let mut out = vec![format!(
                "  ~ kind = {} -> {}",
                recorded.data.kind().name(),
                document.data.kind().name()
            )];
            out.extend(
                entry_bodies(document)
                    .into_iter()
                    .map(|body| format!("  ~ {body}")),
            );
            out
        }
        _ => entry_bodies(document)
            .into_iter()
            .map(|body| format!("  ~ {body}"))
            .collect(),
    }
}

/// Splits a plan key into kind plus path halves.
fn split_key(key: &str) -> (&str, &str) {
    match key.find(':') {
        Some(index) => (&key[..index], &key[index + 1..]),
        None => ("", key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::ids::DocPath;

    #[test]
    fn opaque_update_reuses_content_shape() {
        use confit_core::plan::opaque_label;

        let mut previous_docs = vec![Document::new(
            DocPath::new("bin"),
            DocumentData::Opaque {
                content: vec![0xFF, 0x00],
                mode: None,
            },
        )];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = previous_docs;
        let desired = Document::new(
            DocPath::new("bin"),
            DocumentData::Opaque {
                content: vec![0xFF, 0x01],
                mode: None,
            },
        );
        let built = match confit_core::plan::Plan::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
        };
        let text = report.render();
        let want = format!(
            "  ~ content = {} -> {}",
            opaque_label(&[0xFF, 0x00]),
            opaque_label(&[0xFF, 0x01])
        );
        assert!(text.contains(want.as_str()));
    }

    #[test]
    fn opaque_kind_change_renders_kind_line_plus_body() {
        let mut previous_docs = vec![Document::new(
            DocPath::new("bin"),
            DocumentData::Text {
                content: "hi".to_string(),
                mode: None,
            },
        )];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = previous_docs;
        let desired = Document::new(
            DocPath::new("bin"),
            DocumentData::Opaque {
                content: vec![0xFF, 0x00],
                mode: None,
            },
        );
        let built = match confit_core::plan::Plan::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
        };
        let text = report.render();
        assert!(text.contains("  ~ kind = text -> opaque"));
        assert!(text.contains("  ~ opaque (2 bytes)"));
        assert!(!text.contains("bin: text"));
    }

    #[test]
    fn unchanged_plan_keeps_drift_notes_plus_counts() {
        use confit_core::ids::DocPath;

        let mut previous_docs = vec![Document::new(
            DocPath::new("note"),
            DocumentData::Text {
                content: "hi".to_string(),
                mode: None,
            },
        )];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = previous_docs;
        let desired = Document::new(
            DocPath::new("note"),
            DocumentData::Text {
                content: "hi".to_string(),
                mode: None,
            },
        );
        let built = match confit_core::plan::Plan::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let drift = vec![Drift::Missing {
            path: DocPath::new("note"),
        }];
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &drift,
        };
        let text = report.render();
        assert!(text.contains("manually deleted"));
        assert!(text.contains("Plan: 0 to add, 0 to change, 0 to destroy."));
        assert!(!text.contains("note: text"));
    }

    #[test]
    fn mixed_plan_shows_only_moving_docs() {
        let mut previous_docs = vec![
            Document::new(
                DocPath::new("same"),
                DocumentData::Text {
                    content: "kept".to_string(),
                    mode: None,
                },
            ),
            Document::new(
                DocPath::new("moving"),
                DocumentData::Text {
                    content: "old".to_string(),
                    mode: None,
                },
            ),
        ];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = previous_docs;
        let desired = vec![
            Document::new(
                DocPath::new("same"),
                DocumentData::Text {
                    content: "kept".to_string(),
                    mode: None,
                },
            ),
            Document::new(
                DocPath::new("moving"),
                DocumentData::Text {
                    content: "new".to_string(),
                    mode: None,
                },
            ),
        ];
        let built = match confit_core::plan::Plan::build(desired, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
        };
        let text = report.render();
        assert!(text.contains("moving: text"));
        assert!(!text.contains("same: text"));
    }

    fn tree_member(rel: &str, byte: u8) -> confit_core::document::TreeMember {
        confit_core::document::TreeMember {
            rel: rel.to_string(),
            content: vec![byte],
            mode: 0o644,
        }
    }

    fn tree_previous(members: Vec<confit_core::document::TreeMember>) -> Plan {
        let mut docs = vec![Document::new(
            DocPath::new("fonts"),
            DocumentData::Tree { members },
        )];
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Plan::empty();
        previous.documents = docs;
        previous
    }

    #[test]
    fn tree_create_collapses_to_one_counted_line() {
        let previous = Plan::empty();
        let desired = Document::new(
            DocPath::new("fonts"),
            DocumentData::Tree {
                members: vec![tree_member("a.ttf", 1), tree_member("b.ttf", 2)],
            },
        );
        let built = match confit_core::plan::Plan::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
        };
        let text = report.render();
        assert!(text.contains("fonts: tree"));
        assert!(text.contains("  + tree (2 files)"));
        assert!(text.contains("Plan: 1 to add, 0 to change, 0 to destroy."));
    }

    #[test]
    fn tree_update_counts_changed_members() {
        let previous = tree_previous(vec![tree_member("a.ttf", 1), tree_member("b.ttf", 2)]);
        let desired = Document::new(
            DocPath::new("fonts"),
            DocumentData::Tree {
                members: vec![tree_member("a.ttf", 9), tree_member("b.ttf", 2)],
            },
        );
        let built = match confit_core::plan::Plan::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
        };
        let text = report.render();
        assert!(text.contains("  ~ tree (1 of 2 files changed)"));
        assert!(text.contains("Plan: 0 to add, 1 to change, 0 to destroy."));
    }

    #[test]
    fn tree_delete_names_counted_kind() {
        let previous = tree_previous(vec![tree_member("a.ttf", 1)]);
        let built = match confit_core::plan::Plan::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
        };
        let text = report.render();
        assert!(text.contains("fonts: tree (1 files)"));
        assert!(text.contains("Plan: 0 to add, 0 to change, 1 to destroy."));
    }
}
