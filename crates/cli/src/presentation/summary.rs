//! Summary
//!
//! Stderr summary over built bundles.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use confit_core::document::{DocumentKind, ManifestData, ManifestDocument, RcOp, Table};
use confit_core::drift::{Drift, recorded_hunk};
use confit_core::ids::DocPath;
use confit_core::plan::{Bundle, DocumentStatus};
use confit_core::store::blobs::BlobRef;

/// Yellow style for changed lines.
const UPDATE_STYLE: &str = "\x1b[33m";
/// Green style for added lines.
const ADD_STYLE: &str = "\x1b[32m";
/// Red style for removed lines.
const REMOVE_STYLE: &str = "\x1b[31m";
/// Bold style for headers and the closing counts.
const HEADER_STYLE: &str = "\x1b[1m";
/// Style reset suffix.
const RESET: &str = "\x1b[0m";

/// Drift section title naming manual edits as apply losses.
const DRIFT_TITLE: &str = "Changes outside Confit will be overwritten on next apply";
/// Resources section title over document blocks.
const RESOURCES_TITLE: &str = "Resources";
/// Hooks section title over lifecycle and evaluated lines.
const HOOKS_TITLE: &str = "Hooks";
/// Summary section title over the closing counts.
const SUMMARY_TITLE: &str = "Summary";

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
/// The tty and `NO_COLOR` check runs once under construction,
/// never per line. Piped output stays plain.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::summary::{Painter, Sigil};
///
/// let painter = Painter::new();
/// assert_eq!(painter.paint(Sigil::Add, "hi"), "hi");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Painter {
    /// Paints while stderr runs as a terminal without `NO_COLOR`.
    color: bool,
}

impl Painter {
    /// Reads the color decision from the terminal and the environment.
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

/// One stderr summary over a built bundle and its previous manifest.
///
/// Titled sections carry sigiled headers, empty sections stay
/// out. First runs frame drift as desired versus disk.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::summary::Summary;
/// use confit_core::ids::DocPath;
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::plan::Bundle;
///
/// let document = ManifestDocument::new(
///     DocPath::new("note"),
///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
/// );
/// let built = Bundle::build(vec![document], Vec::new());
/// let previous = Bundle::empty();
/// let summary = match built {
///     Ok(ref built) => Summary { built, previous: &previous, drift: &[], first_run: false, hook_lines: &[], hook_evaluated: &[] },
///     Err(error) => panic!("bundle builds: {error}"),
/// };
/// let text = summary.render();
/// assert!(text.contains("+ note: text"));
/// assert!(text.contains("Documents: 1 to add, 0 to change, 0 to destroy."));
/// assert!(!text.contains("Hooks: "));
/// ```
#[derive(Debug)]
pub struct Summary<'a> {
    /// Holds the built bundle under display.
    pub built: &'a Bundle,
    /// Holds the previous manifest for lifecycle marks.
    pub previous: &'a Bundle,
    /// Holds the manifest versus disk edits leading the text.
    pub drift: &'a [Drift],
    /// Holds true while the state slot reads absent.
    pub first_run: bool,
    /// Holds lifecycle lines for the hooks section.
    pub hook_lines: &'a [String],
    /// Holds evaluated hook lines for the hooks section.
    pub hook_evaluated: &'a [String],
}

impl Summary<'_> {
    /// Renders the full stderr summary with color.
    ///
    /// # Returns
    ///
    /// The stderr summary text.
    pub fn render(&self) -> String {
        if self.first_run {
            return self.render_first_run();
        }
        let painter = Painter::new();
        let mut lines: Vec<String> = Vec::new();
        push_section(
            &mut lines,
            &painter,
            DRIFT_TITLE,
            self.steady_drift_lines(&painter),
        );
        push_section(
            &mut lines,
            &painter,
            RESOURCES_TITLE,
            self.resource_lines(&painter),
        );
        push_section(&mut lines, &painter, HOOKS_TITLE, self.hook_section());
        lines.push(painter.paint(Sigil::Header, SUMMARY_TITLE));
        lines.extend(
            self.summary_lines()
                .iter()
                .map(|line| painter.paint(Sigil::Header, line)),
        );
        lines.join("\n")
    }

    /// Collects resource blocks for steady runs.
    ///
    /// Skips unchanged documents.
    ///
    /// # Returns
    ///
    /// The resource block lines without the section title.
    fn resource_lines(&self, painter: &Painter) -> Vec<String> {
        let mut out = Vec::new();
        for document in &self.built.manifest.documents {
            let status = document.status(self.previous);
            if matches!(status, DocumentStatus::Unchanged) {
                continue;
            }
            out.push(painter.paint(Sigil::Header, &status_header(document, status)));
            out.extend(self.document_lines(painter, document));
        }
        for header in self.delete_headers() {
            out.push(painter.paint(Sigil::Remove, &header));
        }
        out
    }

    /// Collects hook lines for the hooks section.
    ///
    /// # Returns
    ///
    /// The hooks section lines without the section title.
    fn hook_section(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(self.hook_lines.len() + self.hook_evaluated.len());
        out.extend(self.hook_lines.iter().cloned());
        out.extend(self.hook_evaluated.iter().cloned());
        out
    }

    /// Renders the closing counts lines.
    ///
    /// # Returns
    ///
    /// The documents line and the hooks line while hooks move.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::presentation::summary::Summary;
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::Bundle;
    ///
    /// let first = ManifestDocument::new(DocPath::new("a"), ManifestData::Text { content: "a".into(), mode: None, unmanaged: false});
    /// let second = ManifestDocument::new(DocPath::new("b"), ManifestData::Text { content: "b".into(), mode: None, unmanaged: false});
    /// let built = Bundle::build(vec![first, second], Vec::new());
    /// let previous = Bundle::empty();
    /// let summary = match built {
    ///     Ok(ref built) => Summary { built, previous: &previous, drift: &[], first_run: false, hook_lines: &[], hook_evaluated: &[] },
    ///     Err(error) => panic!("bundle builds: {error}"),
    /// };
    /// assert_eq!(
    ///     summary.summary_lines(),
    ///     vec!["Documents: 2 to add, 0 to change, 0 to destroy.".to_string()]
    /// );
    /// ```
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(2);
        if self.first_run {
            let (adds, changes) = self.first_run_counts();
            out.push(format!(
                "Documents: {adds} to add, {changes} to change, 0 to destroy."
            ));
        } else {
            let summary = self.built.summary(self.previous);
            out.push(format!(
                "Documents: {} to add, {} to change, {} to destroy.",
                summary.create, summary.update, summary.delete
            ));
        }
        let (added, changed, destroyed) = hook_counts(self.hook_lines);
        if added + changed + destroyed > 0 {
            out.push(format!(
                "Hooks: {added} to add, {changed} to change, {destroyed} to destroy."
            ));
        }
        out
    }

    /// Counts first-run creates and overwrites.
    ///
    /// Documents holding no drift entries render no lines and
    /// leave the counts.
    ///
    /// # Returns
    ///
    /// The create count and the overwrite count.
    fn first_run_counts(&self) -> (usize, usize) {
        let mut adds = 0;
        let mut changes = 0;
        for document in &self.built.manifest.documents {
            let entries = first_run_entries(self.drift, document);
            if entries.is_empty() {
                continue;
            }
            if first_run_creates(document, &entries) {
                adds += 1;
            } else {
                changes += 1;
            }
        }
        (adds, changes)
    }

    /// Renders the first-run form over desired versus disk drift.
    ///
    /// # Returns
    ///
    /// Resource blocks, hooks, and the counts line.
    fn render_first_run(&self) -> String {
        let painter = Painter::new();
        let mut lines = Vec::new();
        let mut resources = Vec::new();
        for document in &self.built.manifest.documents {
            let entries = first_run_entries(self.drift, document);
            if entries.is_empty() {
                continue;
            }
            let creates = first_run_creates(document, &entries);
            let sigil = if creates { '+' } else { '~' };
            resources
                .push(painter.paint(Sigil::Header, &format!("{sigil} {}", header_line(document))));
            if creates {
                for body in entry_bodies(document) {
                    resources.push(painter.paint(Sigil::Add, &format!("  + {body}")));
                }
            } else {
                resources.extend(first_run_updates(&painter, document, &entries));
            }
        }
        push_section(&mut lines, &painter, RESOURCES_TITLE, resources);
        push_section(&mut lines, &painter, HOOKS_TITLE, self.hook_section());
        lines.push(painter.paint(Sigil::Header, SUMMARY_TITLE));
        lines.extend(
            self.summary_lines()
                .iter()
                .map(|line| painter.paint(Sigil::Header, line)),
        );
        lines.join("\n")
    }

    /// Renders entry lines for one document under its own status.
    fn document_lines(&self, painter: &Painter, document: &ManifestDocument) -> Vec<String> {
        match document.status(self.previous) {
            DocumentStatus::Create => entry_bodies(document)
                .into_iter()
                .map(|body| painter.paint(Sigil::Add, &format!("  + {body}")))
                .collect(),
            DocumentStatus::Update => match self.find_recorded(document) {
                Some(old) => update_lines(painter, document, old),
                None => update_fallback(painter, document),
            },
            DocumentStatus::Unchanged => Vec::new(),
        }
    }

    /// Renders steady drift entries with grouped and painted hunks.
    fn steady_drift_lines(&self, painter: &Painter) -> Vec<String> {
        let mut out = Vec::new();
        for entry in self.drift {
            match entry {
                Drift::Hunk { path, hunks } => {
                    out.push(painter.paint(Sigil::Header, &self.drift_hunk_header(path)));
                    out.extend(painted_hunk_lines(painter, hunks));
                }
                _ => out.extend(Drift::lines(std::slice::from_ref(entry))),
            }
        }
        out
    }

    /// Reads the header line for one steady hunk path.
    fn drift_hunk_header(&self, path: &DocPath) -> String {
        let found = self
            .previous
            .manifest
            .documents
            .iter()
            .find(|item| item.path == *path)
            .or_else(|| {
                self.built
                    .manifest
                    .documents
                    .iter()
                    .find(|item| item.path == *path)
            });
        match found {
            Some(document) => format!("~ {}", header_line(document)),
            None => format!("~ {}", path.as_str()),
        }
    }

    /// Renders delete headers for previous keys missing from the manifest.
    fn delete_headers(&self) -> Vec<String> {
        let seen: BTreeSet<String> = self
            .built
            .manifest
            .documents
            .iter()
            .map(|document| document.key())
            .collect();
        self.previous
            .manifest
            .documents
            .iter()
            .filter(|recorded| {
                !seen.contains(&recorded.key())
                    && !recorded.superseded_by(&self.built.manifest.documents)
            })
            .map(|recorded| {
                let key = recorded.key();
                let (kind, path) = split_key(&key);
                match &recorded.data {
                    ManifestData::Tree { members } => {
                        format!("- {path}: {kind} ({} files)", members.len())
                    }
                    _ => format!("- {path}: {kind}"),
                }
            })
            .collect()
    }

    /// Finds one recorded document by key with opaque fallback.
    fn find_recorded<'a>(&'a self, document: &ManifestDocument) -> Option<&'a ManifestDocument> {
        if let Some(found) = self
            .previous
            .manifest
            .documents
            .iter()
            .find(|item| item.key() == document.key())
        {
            return Some(found);
        }
        self.previous.manifest.documents.iter().find(|recorded| {
            recorded.path == document.path
                && recorded.key() != document.key()
                && (recorded.is_opaque() || document.is_opaque())
        })
    }
}

/// Reads the display label for one document.
fn doc_label(document: &ManifestDocument) -> Cow<'_, str> {
    match &document.data {
        ManifestData::Structured { format, .. } => Cow::Borrowed(format.name()),
        _ => Cow::Borrowed(document.data.kind().name()),
    }
}

/// Reads the header line for one document.
fn header_line(document: &ManifestDocument) -> String {
    format!("{}: {}", document.path.as_str(), doc_label(document))
}

/// Formats one document header with its lifecycle sigil.
fn status_header(document: &ManifestDocument, status: DocumentStatus) -> String {
    let sigil = match status {
        DocumentStatus::Create => '+',
        DocumentStatus::Update => '~',
        DocumentStatus::Unchanged => ' ',
    };
    format!("{sigil} {}", header_line(document))
}

/// Appends one titled section while its body holds lines.
///
/// Empty bodies render no title.
fn push_section(lines: &mut Vec<String>, painter: &Painter, title: &str, body: Vec<String>) {
    if body.is_empty() {
        return;
    }
    lines.push(painter.paint(Sigil::Header, title));
    lines.extend(body);
}

/// Counts lifecycle lines by top-level sigil.
///
/// Detail lines nest under spaces and never count. The triple
/// describes hook records in the bundle, never execution.
fn hook_counts(lines: &[String]) -> (usize, usize, usize) {
    let mut added = 0;
    let mut changed = 0;
    let mut destroyed = 0;
    for line in lines {
        if line.starts_with('+') {
            added += 1;
        } else if line.starts_with('~') {
            changed += 1;
        } else if line.starts_with('-') {
            destroyed += 1;
        }
    }
    (added, changed, destroyed)
}

/// Collects one document's drift entries for first runs.
///
/// Tree member entries group under their destination path.
/// Every other entry groups under its own path.
fn first_run_entries<'a>(drift: &'a [Drift], document: &ManifestDocument) -> Vec<&'a Drift> {
    let dest = document.path.as_str();
    let is_tree = document.data.tree_members().is_some();
    drift
        .iter()
        .filter(|entry| {
            let path = match entry {
                Drift::Key { path, .. }
                | Drift::Hunk { path, .. }
                | Drift::Missing { path }
                | Drift::Unreadable { path, .. } => path.as_str(),
            };
            if path == dest {
                return true;
            }
            is_tree
                && path
                    .strip_prefix(dest)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .collect()
}

/// Reports whether one first-run group lands whole.
///
/// Whole-file missing entries always land whole. Whole trees
/// holding missing entries alone land whole.
fn first_run_creates(document: &ManifestDocument, entries: &[&Drift]) -> bool {
    if entries
        .iter()
        .any(|entry| matches!(entry, Drift::Missing { path } if path == &document.path))
    {
        return true;
    }
    if document.data.tree_members().is_some() {
        return entries
            .iter()
            .all(|entry| matches!(entry, Drift::Missing { .. }));
    }
    false
}

/// Renders one first-run update group with disk values first.
///
/// Structured, link, and opaque keys read disk to desired.
/// Text and rc hunks render verbatim with per-line paint.
/// Trees collapse to one changed member count.
/// Unreadable paths name the replacement.
fn first_run_updates(
    painter: &Painter,
    document: &ManifestDocument,
    entries: &[&Drift],
) -> Vec<String> {
    if let Some(members) = document.data.tree_members() {
        let mut rels = BTreeSet::new();
        for entry in entries {
            rels.insert(member_rel(document, entry));
        }
        return vec![painter.paint(
            Sigil::Update,
            &format!(
                "  ~ tree ({} of {} files changed)",
                rels.len(),
                members.len()
            ),
        )];
    }
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Drift::Key { key, old, new, .. } => match (old, new) {
                (Some(old_value), Some(new_value)) => out.push(painter.paint(
                    Sigil::Update,
                    &format!(
                        "  ~ {key} = {} -> {}",
                        leaf_text(old_value),
                        leaf_text(new_value)
                    ),
                )),
                (Some(old_value), None) => out.push(painter.paint(
                    Sigil::Remove,
                    &format!("  - {key} = {}", leaf_text(old_value)),
                )),
                (None, Some(new_value)) => out.push(
                    painter.paint(Sigil::Add, &format!("  + {key} = {}", leaf_text(new_value))),
                ),
                (None, None) => {}
            },
            Drift::Hunk { hunks, .. } => {
                out.extend(painted_hunk_lines(painter, hunks));
            }
            Drift::Unreadable { reason, .. } => out.push(painter.paint(
                Sigil::Update,
                &format!("  ~ unreadable ({reason}), apply will write desired content"),
            )),
            Drift::Missing { .. } => {}
        }
    }
    out
}

/// Paints unified hunk content lines by their leading symbol.
fn painted_hunk_lines(painter: &Painter, hunks: &str) -> Vec<String> {
    hunks
        .lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .map(|line| paint_hunk_line(painter, line))
        .collect()
}

/// Paints one hunk content line by its leading symbol.
fn paint_hunk_line(painter: &Painter, line: &str) -> String {
    if let Some(rest) = line.strip_prefix('+') {
        painter.paint(Sigil::Add, &format!("  +{rest}"))
    } else if let Some(rest) = line.strip_prefix('-') {
        painter.paint(Sigil::Remove, &format!("  -{rest}"))
    } else {
        format!("  {}", line.strip_prefix(' ').unwrap_or(line))
    }
}

/// Reads one entry's tree member rel under its destination.
fn member_rel<'a>(document: &'a ManifestDocument, entry: &'a Drift) -> &'a str {
    let dest = document.path.as_str();
    match entry {
        Drift::Key { key, .. } => key.strip_suffix(":mode").unwrap_or(key),
        Drift::Hunk { path, .. } | Drift::Missing { path } | Drift::Unreadable { path, .. } => path
            .as_str()
            .strip_prefix(dest)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(path.as_str()),
    }
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
                out.extend(collect_value(&format!("{key}[{}]", index + 1), item));
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
fn entry_bodies(document: &ManifestDocument) -> Vec<String> {
    match &document.data {
        ManifestData::Structured { data, .. } => table_leaves(data, "")
            .into_iter()
            .map(|(key, value)| format!("{key} = {value}"))
            .collect(),
        ManifestData::Text { content, .. } => {
            if content.is_empty() {
                Vec::new()
            } else {
                content.split('\n').map(str::to_string).collect()
            }
        }
        ManifestData::Link { target } => vec![target.clone()],
        ManifestData::Opaque { size, .. } => {
            vec![format!("opaque ({size} bytes)")]
        }
        ManifestData::Tree { members } => vec![format!("tree ({} files)", members.len())],
        ManifestData::Rc(rc) => {
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
fn touches_opaque(first: &ManifestDocument, second: &ManifestDocument) -> bool {
    matches!(first.data.kind(), DocumentKind::Opaque)
        || matches!(second.data.kind(), DocumentKind::Opaque)
}

/// Reads the hash and size label for one opaque ref.
///
/// Refs carry the content hash and byte count, so the label
/// matches `opaque_label` without reading blob bytes.
fn opaque_ref_label(sha: &str, size: u64) -> String {
    format!("sha256:{sha} ({size} bytes)")
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
                out.extend(flatten_json(&format!("{key}[{}]", index + 1), item));
            }
        }
        _ => {
            out.insert(key.to_string(), value.clone());
        }
    }
    out
}

/// Collects update lines with old to new values.
///
/// Structured, link, opaque, and tree lines read yellow
/// under the update sigil. Opaque labels read the recorded hash
/// and size, never blob bytes. Rc updates read as a recorded to
/// desired text hunk with per-symbol paint.
fn update_lines(
    painter: &Painter,
    document: &ManifestDocument,
    recorded: &ManifestDocument,
) -> Vec<String> {
    match (&document.data, &recorded.data) {
        (
            ManifestData::Structured { data: new, .. },
            ManifestData::Structured { data: old, .. },
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
                let old_value = old_flat.get(&key).cloned();
                let new_value = new_flat.get(&key).cloned();
                if old_value != new_value {
                    match (&old_value, &new_value) {
                        (Some(old_text), Some(new_text)) => out.push(painter.paint(
                            Sigil::Update,
                            &format!(
                                "  ~ {key} = {} -> {}",
                                leaf_text(old_text),
                                leaf_text(new_text)
                            ),
                        )),
                        (Some(old_text), None) => out.push(painter.paint(
                            Sigil::Remove,
                            &format!("  - {key} = {}", leaf_text(old_text)),
                        )),
                        (None, Some(new_text)) => out.push(
                            painter
                                .paint(Sigil::Add, &format!("  + {key} = {}", leaf_text(new_text))),
                        ),
                        (None, None) => {}
                    }
                }
            }
            out
        }
        (ManifestData::Link { target: new }, ManifestData::Link { target: old }) => {
            if old == new {
                Vec::new()
            } else {
                vec![painter.paint(Sigil::Update, &format!("  ~ target = {old} -> {new}"))]
            }
        }
        (
            ManifestData::Opaque {
                blob: new_blob,
                size: new_size,
                ..
            },
            ManifestData::Opaque {
                blob: old_blob,
                size: old_size,
                ..
            },
        ) => {
            if old_blob == new_blob {
                Vec::new()
            } else {
                vec![painter.paint(
                    Sigil::Update,
                    &format!(
                        "  ~ content = {} -> {}",
                        opaque_ref_label(old_blob, *old_size),
                        opaque_ref_label(new_blob, *new_size)
                    ),
                )]
            }
        }
        (ManifestData::Tree { members: new }, ManifestData::Tree { members: old }) => {
            let changed = confit_core::document::tree_changed(old, new);
            vec![painter.paint(
                Sigil::Update,
                &format!("  ~ tree ({changed} of {} files changed)", new.len()),
            )]
        }
        (ManifestData::Rc(_), ManifestData::Rc(_)) => rc_update_lines(painter, document, recorded),
        _ if touches_opaque(document, recorded) => {
            let mut out = vec![painter.paint(
                Sigil::Update,
                &format!(
                    "  ~ kind = {} -> {}",
                    recorded.data.kind().name(),
                    document.data.kind().name()
                ),
            )];
            out.extend(update_fallback(painter, document));
            out
        }
        _ => update_fallback(painter, document),
    }
}

/// Renders desired entry bodies under the update sigil.
fn update_fallback(painter: &Painter, document: &ManifestDocument) -> Vec<String> {
    entry_bodies(document)
        .into_iter()
        .map(|body| painter.paint(Sigil::Update, &format!("  ~ {body}")))
        .collect()
}

/// Renders one rc update as a recorded to desired text hunk.
///
/// Rc payloads render inline, so no blob map reads. Hunk lines
/// carry per-symbol paint through the shared hunk path. Render
/// failures fall back to desired entry bodies under the update
/// sigil.
fn rc_update_lines(
    painter: &Painter,
    document: &ManifestDocument,
    recorded: &ManifestDocument,
) -> Vec<String> {
    let empty: BTreeMap<String, BlobRef> = BTreeMap::new();
    let old_bytes = match recorded.render(&empty) {
        Ok(bytes) => bytes,
        Err(_) => return update_fallback(painter, document),
    };
    let new_bytes = match document.render(&empty) {
        Ok(bytes) => bytes,
        Err(_) => return update_fallback(painter, document),
    };
    if old_bytes == new_bytes {
        return Vec::new();
    }
    let old = String::from_utf8_lossy(&old_bytes);
    let new = String::from_utf8_lossy(&new_bytes);
    let hunks = recorded_hunk(&old, &new);
    painted_hunk_lines(painter, &hunks)
}

/// Splits a manifest key into kind and path halves.
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

        let old_blob = confit_core::ids::sha256_hex(&[0xFF, 0x00]);
        let new_blob = confit_core::ids::sha256_hex(&[0xFF, 0x01]);
        let mut previous_docs = vec![ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: old_blob.clone(),
                size: 2,
                mode: None,
                unmanaged: false,
            },
        )];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = previous_docs;
        let desired = ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: new_blob.clone(),
                size: 2,
                mode: None,
                unmanaged: false,
            },
        );
        let built = match confit_core::plan::Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
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
        let mut previous_docs = vec![ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Text {
                content: "hi".to_string(),
                mode: None,
                unmanaged: false,
            },
        )];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = previous_docs;
        let blob = confit_core::ids::sha256_hex(&[0xFF, 0x00]);
        let desired = ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: blob.clone(),
                size: 2,
                mode: None,
                unmanaged: false,
            },
        );
        let built = match confit_core::plan::Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("  ~ kind = text -> opaque"));
        assert!(text.contains("  ~ opaque (2 bytes)"));
        assert!(!text.contains("bin: text"));
    }

    #[test]
    fn unchanged_plan_keeps_drift_notes_plus_counts() {
        use confit_core::ids::DocPath;

        let mut previous_docs = vec![ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "hi".to_string(),
                mode: None,
                unmanaged: false,
            },
        )];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = previous_docs;
        let desired = ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "hi".to_string(),
                mode: None,
                unmanaged: false,
            },
        );
        let built = match confit_core::plan::Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let drift = vec![Drift::Missing {
            path: DocPath::new("note"),
        }];
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &drift,
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("manually deleted"));
        assert!(text.contains("Documents: 0 to add, 0 to change, 0 to destroy."));
        assert!(!text.contains("note: text"));
    }

    #[test]
    fn mixed_plan_shows_only_moving_docs() {
        let mut previous_docs = vec![
            ManifestDocument::new(
                DocPath::new("same"),
                ManifestData::Text {
                    content: "kept".to_string(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("moving"),
                ManifestData::Text {
                    content: "old".to_string(),
                    mode: None,
                    unmanaged: false,
                },
            ),
        ];
        for document in &mut previous_docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = previous_docs;
        let desired = vec![
            ManifestDocument::new(
                DocPath::new("same"),
                ManifestData::Text {
                    content: "kept".to_string(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("moving"),
                ManifestData::Text {
                    content: "new".to_string(),
                    mode: None,
                    unmanaged: false,
                },
            ),
        ];
        let built = match confit_core::plan::Bundle::build(desired, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("Resources"), "resources title shows: {text}");
        assert!(text.contains("Summary"), "summary title shows: {text}");
        assert!(text.contains("~ moving: text"));
        assert!(!text.contains("same: text"));
    }

    fn tree_member(relative: &str, byte: u8) -> confit_core::document::ManifestMember {
        confit_core::document::ManifestMember {
            relative: relative.to_string(),
            blob: confit_core::ids::sha256_hex(&[byte]),
            size: 1,
            mode: 0o644,
        }
    }

    fn tree_previous(members: Vec<confit_core::document::ManifestMember>) -> Bundle {
        let mut docs = vec![ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree { members },
        )];
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = docs;
        previous
    }

    #[test]
    fn tree_create_collapses_to_one_counted_line() {
        let previous = Bundle::empty();
        let desired = ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree {
                members: vec![tree_member("a.ttf", 1), tree_member("b.ttf", 2)],
            },
        );
        let built = match confit_core::plan::Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("+ fonts: tree"));
        assert!(text.contains("  + tree (2 files)"));
        assert!(text.contains("Documents: 1 to add, 0 to change, 0 to destroy."));
    }

    #[test]
    fn tree_update_counts_changed_members() {
        let previous = tree_previous(vec![tree_member("a.ttf", 1), tree_member("b.ttf", 2)]);
        let desired = ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree {
                members: vec![tree_member("a.ttf", 9), tree_member("b.ttf", 2)],
            },
        );
        let built = match confit_core::plan::Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("  ~ tree (1 of 2 files changed)"));
        assert!(text.contains("Documents: 0 to add, 1 to change, 0 to destroy."));
    }

    #[test]
    fn tree_delete_names_counted_kind() {
        let previous = tree_previous(vec![tree_member("a.ttf", 1)]);
        let built = match confit_core::plan::Bundle::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("- fonts: tree (1 files)"));
        assert!(text.contains("Documents: 0 to add, 0 to change, 1 to destroy."));
    }

    #[test]
    fn first_run_renders_compact_doc_lines() {
        use confit_core::drift::Drift;
        use confit_core::ids::DocPath;

        let built = match confit_core::plan::Bundle::build(
            vec![
                ManifestDocument::new(
                    DocPath::new("same"),
                    ManifestData::Text {
                        content: "kept".to_string(),
                        mode: None,
                        unmanaged: false,
                    },
                ),
                ManifestDocument::new(
                    DocPath::new("gone"),
                    ManifestData::Text {
                        content: "fresh".to_string(),
                        mode: None,
                        unmanaged: false,
                    },
                ),
                ManifestDocument::new(
                    DocPath::new("app.toml"),
                    ManifestData::Structured {
                        format: confit_core::document::StructuredFormat::Toml,
                        data: [("name".to_string(), serde_json::json!("desired"))]
                            .into_iter()
                            .collect(),
                    },
                ),
                ManifestDocument::new(
                    DocPath::new("fonts"),
                    ManifestData::Tree {
                        members: vec![tree_member("a.ttf", 1), tree_member("b.ttf", 2)],
                    },
                ),
                ManifestDocument::new(
                    DocPath::new("clash"),
                    ManifestData::Text {
                        content: "desired\n".to_string(),
                        mode: None,
                        unmanaged: false,
                    },
                ),
            ],
            Vec::new(),
        ) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let previous = Bundle::empty();
        let drift = vec![
            Drift::Missing {
                path: DocPath::new("gone"),
            },
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "name".to_string(),
                old: Some(serde_json::json!("disk")),
                new: Some(serde_json::json!("desired")),
            },
            Drift::Key {
                path: DocPath::new("fonts"),
                key: "b.ttf".to_string(),
                old: Some(serde_json::json!("new")),
                new: Some(serde_json::json!("old")),
            },
            Drift::Missing {
                path: DocPath::new("fonts/a.ttf"),
            },
            Drift::Hunk {
                path: DocPath::new("clash"),
                hunks: "--- disk\n+++ desired\n@@ -1 +1 @@\n-disk\n+desired".to_string(),
            },
        ];
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &drift,
            first_run: true,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("+ gone: text"), "create header shows: {text}");
        assert!(text.contains("  + fresh"), "create body shows: {text}");
        assert!(
            text.contains("~ app.toml: toml"),
            "overwrite header shows: {text}"
        );
        assert!(
            text.contains("  ~ name = disk -> desired"),
            "key swaps to disk first: {text}"
        );
        assert!(
            text.contains("  ~ tree (2 of 2 files changed)"),
            "tree collapses to one line: {text}"
        );
        assert!(
            !text.contains("same"),
            "in place stays out of lines: {text}"
        );
        assert!(
            text.contains("~ clash: text"),
            "text overwrite header shows: {text}"
        );
        assert!(text.contains("-disk"), "hunk shows disk line: {text}");
        assert!(text.contains("+desired"), "hunk shows desired line: {text}");
        assert!(text.contains("Documents: 1 to add, 3 to change, 0 to destroy."));
        assert!(!text.contains("changed outside config"));
    }

    #[test]
    fn hunk_lines_drop_markers() {
        let painter = Painter { color: true };
        let hunks = "--- disk\n+++ desired\n@@ -1 +1 @@\n-disk\n context\n+desired";
        let painted = painted_hunk_lines(&painter, hunks);
        assert!(
            !painted.iter().any(|line| {
                line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@")
            }),
            "markers never render: {painted:?}"
        );
        assert!(
            painted
                .iter()
                .any(|line| line.starts_with("\x1b[31m") && line.contains("-disk")),
            "removals read red: {painted:?}"
        );
        assert!(
            painted.iter().any(|line| line == "  context"),
            "context keeps entry indent: {painted:?}"
        );
        assert!(
            painted
                .iter()
                .any(|line| line.starts_with("\x1b[32m") && line.contains("+desired")),
            "additions read green: {painted:?}"
        );
    }

    #[test]
    fn hunk_lines_carry_entry_indent_with_aligned_code() {
        let painter = Painter { color: false };
        let hunks = "@@ -1,3 +1,3 @@\n if kept; then\n-  old\n+  new\n fi";
        let painted = painted_hunk_lines(&painter, hunks);
        assert_eq!(
            painted,
            vec!["  if kept; then", "  -  old", "  +  new", "  fi"]
        );
    }

    #[test]
    fn destroy_headers_carry_bare_remove_marks() {
        let previous = tree_previous(vec![tree_member("a.ttf", 1)]);
        let built = match confit_core::plan::Bundle::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(text.contains("- fonts: tree (1 files)"));
        assert!(!text.contains("  - fonts"));
    }

    #[test]
    fn hooks_section_counts_added_plus_changed_as_to_run() {
        let previous = Bundle::empty();
        let built = match Bundle::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let hook_lines = vec![
            "+ mise install".to_string(),
            "  + requires (in_path(mise))".to_string(),
            "~ fc-cache -f fonts".to_string(),
            "- old hook".to_string(),
        ];
        let evaluated = vec!["! run: /home/tester/.local/bin/mise install".to_string()];
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &hook_lines,
            hook_evaluated: &evaluated,
        };
        let text = report.render();
        assert!(text.contains("Hooks"), "hooks title shows: {text}");
        assert!(text.contains("+ mise install"), "hook header shows: {text}");
        assert!(
            text.contains("! run: /home/tester/.local/bin/mise install"),
            "evaluated line trails: {text}"
        );
        assert!(
            text.contains("Documents: 0 to add, 0 to change, 0 to destroy."),
            "documents line closes: {text}"
        );
        assert!(
            text.contains("Hooks: 1 to add, 1 to change, 1 to destroy."),
            "every sigil counts: {text}"
        );
    }

    #[test]
    fn hook_triple_counts_top_level_sigils() {
        let previous = Bundle::empty();
        let built = match Bundle::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let hook_lines = vec!["~ mise install".to_string()];
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &hook_lines,
            hook_evaluated: &[],
        };
        assert!(
            report
                .summary_lines()
                .contains(&"Hooks: 0 to add, 1 to change, 0 to destroy.".to_string()),
            "changed hook counts: {:?}",
            report.summary_lines()
        );
    }

    #[test]
    fn quiet_plan_closes_with_summary_alone() {
        let previous = Bundle::empty();
        let built = match Bundle::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(
            !text.contains("Resources"),
            "empty resources stay quiet: {text}"
        );
        assert!(!text.contains("Hooks"), "empty hooks stay quiet: {text}");
        assert!(text.contains("Summary"), "summary always closes: {text}");
    }

    #[test]
    fn structured_arrays_render_one_based_keys() {
        use confit_core::document::StructuredFormat;

        fn servers(hosts: &[&str]) -> ManifestDocument {
            let items: Vec<serde_json::Value> = hosts
                .iter()
                .map(|host| serde_json::json!({"host": host}))
                .collect();
            ManifestDocument::new(
                DocPath::new("app.json"),
                ManifestData::Structured {
                    format: StructuredFormat::Json,
                    data: [("servers".to_string(), serde_json::Value::Array(items))]
                        .into_iter()
                        .collect(),
                },
            )
        }

        let previous = hashed_docs(vec![servers(&["a", "b"])]);
        let fresh = ManifestDocument::new(
            DocPath::new("fresh.json"),
            ManifestData::Structured {
                format: StructuredFormat::Json,
                data: [("servers".to_string(), serde_json::json!([{"host": "a"}]))]
                    .into_iter()
                    .collect(),
            },
        );
        let built = match Bundle::build(vec![servers(&["a", "c"]), fresh], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(
            text.contains("~ servers[2].host = b -> c"),
            "updates count from one: {text}"
        );
        assert!(
            text.contains("+ servers[1].host = a"),
            "creates count from one: {text}"
        );
        assert!(!text.contains("[0]"), "no zero key leaks: {text}");
    }

    fn hashed_docs(documents: Vec<ManifestDocument>) -> Bundle {
        let mut docs = documents;
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = docs;
        previous
    }

    fn text_document(path: &str, content: &str) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Text {
                content: content.to_string(),
                mode: None,
                unmanaged: false,
            },
        )
    }

    fn rc_entry(op: RcOp) -> confit_core::document::RcEntry {
        confit_core::document::RcEntry { op, when: None }
    }

    fn rc_document(path: &str, profile: Vec<confit_core::document::RcEntry>) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Rc(confit_core::document::RcData::new(
                profile,
                Vec::new(),
                Vec::new(),
            )),
        )
    }

    #[test]
    fn steady_hunk_groups_under_header_with_symbol_paint() {
        use confit_core::drift::DriftOrder;
        use confit_core::ids::ReadOutcome;

        let previous = hashed_docs(vec![text_document("note", "recorded\n")]);
        let built = match Bundle::build(vec![text_document("note", "recorded\n")], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let drift = previous.drift(
            &|_| ReadOutcome::Present {
                bytes: b"disk\n".to_vec(),
                mode: None,
            },
            &|_| std::collections::BTreeMap::new(),
            DriftOrder::RecordedFirst,
            &confit_core::fs::memory::MemoryFs::new(),
        );
        assert_eq!(drift.len(), 1);
        let hunks = match &drift[0] {
            Drift::Hunk { hunks, .. } => hunks.clone(),
            _ => panic!("steady drift pins hunk"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &drift,
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(
            text.contains("note: text"),
            "steady hunk groups under document header: {text}"
        );
        assert!(
            !text.contains("---"),
            "steady summary renders no file markers: {text}"
        );
        assert!(
            !text.contains("+++"),
            "steady summary renders no new markers: {text}"
        );
        assert!(
            !text.contains("@@"),
            "steady summary renders no range markers: {text}"
        );
        assert!(
            text.contains("-recorded"),
            "steady removal shows content: {text}"
        );
        assert!(
            text.contains("+disk"),
            "steady addition shows content: {text}"
        );
        let header_at = match text.find("note: text") {
            Some(at) => at,
            None => panic!("steady header shows: {text}"),
        };
        let hunk_at = match text.find("-recorded") {
            Some(at) => at,
            None => panic!("steady hunk shows: {text}"),
        };
        assert!(header_at < hunk_at, "hunk groups under its header: {text}");
        let painter = Painter { color: true };
        let painted = painted_hunk_lines(&painter, &hunks);
        let mut saw_red = false;
        let mut saw_green = false;
        for line in &painted {
            if line.contains("-recorded") {
                assert!(
                    line.starts_with("\x1b[31m"),
                    "steady removal reads red: {line}"
                );
                saw_red = true;
            }
            if line.contains("+disk") && !line.starts_with("+++") {
                assert!(
                    line.starts_with("\x1b[32m"),
                    "steady addition reads green: {line}"
                );
                saw_green = true;
            }
            if line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@") {
                assert!(
                    !line.starts_with("\x1b["),
                    "steady markers stay plain: {line}"
                );
            }
        }
        assert!(saw_red, "steady paint covers removals");
        assert!(saw_green, "steady paint covers additions");
        let bold = painter.paint(Sigil::Header, "note: text");
        assert!(
            bold.starts_with("\x1b[1m"),
            "steady header reads bold: {bold}"
        );
    }

    #[test]
    fn steady_non_hunk_lines_stay_byte_identical() {
        let previous = hashed_docs(vec![text_document("note", "hi")]);
        let built = match Bundle::build(vec![text_document("note", "hi")], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let drift = vec![
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.bat".to_string(),
                old: Some(serde_json::json!("old")),
                new: Some(serde_json::json!("new")),
            },
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.gone".to_string(),
                old: Some(serde_json::json!("old")),
                new: None,
            },
            Drift::Key {
                path: DocPath::new("app.toml"),
                key: "tools.fresh".to_string(),
                old: None,
                new: Some(serde_json::json!("new")),
            },
            Drift::Missing {
                path: DocPath::new("gone"),
            },
        ];
        let want = Drift::lines(&drift);
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &drift,
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        for line in &want {
            assert!(
                text.contains(line.as_str()),
                "non-hunk line stays byte-identical: {line} in {text}"
            );
        }
    }

    #[test]
    fn rc_update_renders_recorded_to_desired_hunk() {
        let recorded = rc_document(
            "~/.bashrc",
            vec![
                rc_entry(RcOp::Env {
                    name: "SHARED".to_string(),
                    value: "same".to_string(),
                }),
                rc_entry(RcOp::Env {
                    name: "CHANGED".to_string(),
                    value: "old".to_string(),
                }),
            ],
        );
        let desired = rc_document(
            "~/.bashrc",
            vec![
                rc_entry(RcOp::Env {
                    name: "SHARED".to_string(),
                    value: "same".to_string(),
                }),
                rc_entry(RcOp::Env {
                    name: "CHANGED".to_string(),
                    value: "new".to_string(),
                }),
            ],
        );
        let previous = hashed_docs(vec![recorded]);
        let built = match Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(
            !text.contains("---"),
            "rc update renders no file markers: {text}"
        );
        assert!(
            !text.contains("+++"),
            "rc update renders no new markers: {text}"
        );
        assert!(text.contains("-"), "rc update removes old: {text}");
        assert!(text.contains("+"), "rc update adds new: {text}");
        let painter = Painter { color: true };
        let hunks = confit_core::drift::recorded_hunk("old\n", "new\n");
        let painted = painted_hunk_lines(&painter, &hunks);
        let mut saw_red = false;
        let mut saw_green = false;
        for line in &painted {
            if line.contains("-old") {
                assert!(line.starts_with("\x1b[31m"), "rc removal reads red: {line}");
                saw_red = true;
            }
            if line.contains("+new") && !line.starts_with("+++") {
                assert!(
                    line.starts_with("\x1b[32m"),
                    "rc addition reads green: {line}"
                );
                saw_green = true;
            }
        }
        assert!(saw_red, "rc paint covers removals");
        assert!(saw_green, "rc paint covers additions");
    }

    #[test]
    fn rc_identical_renders_no_lines() {
        let profile = vec![rc_entry(RcOp::Env {
            name: "SHARED".to_string(),
            value: "same".to_string(),
        })];
        let previous = hashed_docs(vec![rc_document("~/.bashrc", profile.clone())]);
        let built = match Bundle::build(vec![rc_document("~/.bashrc", profile)], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        assert!(
            !text.contains("--- recorded"),
            "identical rc renders no hunk: {text}"
        );
        assert!(
            !text.contains("+++ desired"),
            "identical rc renders no desired marker: {text}"
        );
        assert!(
            !text.contains("~/.bashrc"),
            "identical rc renders no document block: {text}"
        );
    }

    #[test]
    fn rc_shared_entries_never_render_as_changes() {
        let recorded = rc_document(
            "~/.bashrc",
            vec![
                rc_entry(RcOp::Env {
                    name: "SHARED".to_string(),
                    value: "same".to_string(),
                }),
                rc_entry(RcOp::Env {
                    name: "CHANGED".to_string(),
                    value: "old".to_string(),
                }),
            ],
        );
        let desired = rc_document(
            "~/.bashrc",
            vec![
                rc_entry(RcOp::Env {
                    name: "SHARED".to_string(),
                    value: "same".to_string(),
                }),
                rc_entry(RcOp::Env {
                    name: "CHANGED".to_string(),
                    value: "new".to_string(),
                }),
            ],
        );
        let previous = hashed_docs(vec![recorded]);
        let built = match Bundle::build(vec![desired], Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let report = Summary {
            built: &built,
            previous: &previous,
            drift: &[],
            first_run: false,
            hook_lines: &[],
            hook_evaluated: &[],
        };
        let text = report.render();
        for line in text.lines() {
            let changed = (line.starts_with('-') && !line.starts_with("---"))
                || (line.starts_with('+') && !line.starts_with("+++"))
                || (line.contains("\x1b[31m") && line.contains("SHARED"))
                || (line.contains("\x1b[32m") && line.contains("SHARED"));
            if changed {
                assert!(
                    !line.contains("SHARED"),
                    "shared entry never renders as a change: {line} in {text}"
                );
            }
        }
        assert!(
            text.contains("CHANGED"),
            "changed entry renders the diff: {text}"
        );
    }

    #[test]
    fn opaque_label_renders_from_size_without_blobs() {
        let document = ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: "missing".to_string(),
                size: 5,
                mode: None,
                unmanaged: false,
            },
        );
        assert_eq!(
            entry_bodies(&document),
            vec!["opaque (5 bytes)".to_string()]
        );
    }
}
