//! Summary
//!
//! Stderr summary over built bundles.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use confit_core::arg::Arg;
use confit_core::document::{DocumentKind, ManifestData, ManifestDocument, RcOp, Table};
use confit_core::drift::Drift;
use confit_core::handles::{Route, Sha};
use confit_core::hook::HookLifecycle;
use confit_core::plan::{Bundle, DocumentStatus};

use crate::presentation::drift::drift_lines;
use crate::presentation::hooks::{EvaluatedHook, lifecycle_lines, render_evaluated};

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

impl Sigil {
    /// Reads the line mark for one sigil.
    ///
    /// Headers carry no lifecycle mark, so the mark reads blank.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::presentation::summary::Sigil;
    ///
    /// assert_eq!(Sigil::Add.mark(), '+');
    /// assert_eq!(Sigil::Header.mark(), ' ');
    /// ```
    pub fn mark(self) -> char {
        match self {
            Sigil::Add => '+',
            Sigil::Update => '~',
            Sigil::Remove => '-',
            Sigil::Header => ' ',
        }
    }
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

/// Hook display data for one summary run.
///
/// Lifecycle entries render lifecycle lines, evaluated hooks
/// render trailing preview lines. Core owns the hooks,
/// presentation owns every rendered line.
#[derive(Debug)]
pub struct Hooks<'a> {
    /// Holds the lifecycle entries in plan order with removals trailing.
    pub lifecycle: &'a [HookLifecycle<'a>],
    /// Holds the evaluated hooks trailing the lifecycle lines.
    pub evaluated: &'a [EvaluatedHook<'a>],
}

/// One hook count triple for one summary run.
///
/// Added counts added headers, changed counts changed headers,
/// destroyed counts removed headers. Detail lines never count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookCounts {
    /// Counts added hook headers.
    pub added: usize,
    /// Counts changed hook headers.
    pub changed: usize,
    /// Counts removed hook headers.
    pub destroyed: usize,
}

impl HookCounts {
    /// Reports whether one triple holds no moving hooks.
    ///
    /// # Returns
    ///
    /// True while added, changed, and destroyed all read zero.
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.destroyed == 0
    }

    /// Reads the total moving hooks behind one triple.
    ///
    /// # Returns
    ///
    /// The added, changed, and destroyed counts joined.
    pub fn total(&self) -> usize {
        self.added + self.changed + self.destroyed
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
/// use confit_cli::presentation::summary::{Hooks, Summary};
/// use confit_core::handles::{Route, RouteBase};
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::plan::Bundle;
///
/// let document = ManifestDocument::new(
///     Route::new(RouteBase::Home, "note").unwrap(),
///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
/// );
/// let built = Bundle::build(vec![document], Vec::new());
/// let previous = Bundle::empty();
/// let summary = match built {
///     Ok(ref built) => Summary { built, previous: &previous, drift: &[], first_run: false, hooks: Hooks { lifecycle: &[], evaluated: &[] } },
///     Err(error) => panic!("bundle builds: {error}"),
/// };
/// let text = summary.render();
/// assert!(text.contains("+ home:note: text"));
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
    /// Holds the hook display data for the hooks section.
    pub hooks: Hooks<'a>,
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

    /// Renders hook lifecycle lines from display data.
    ///
    /// # Returns
    ///
    /// The lifecycle lines in plan order with removals trailing.
    fn hook_lifecycle(&self) -> Vec<String> {
        lifecycle_lines(self.hooks.lifecycle)
    }

    /// Collects hook lines for the hooks section.
    ///
    /// # Returns
    ///
    /// The hooks section lines without the section title.
    fn hook_section(&self) -> Vec<String> {
        let mut out = self.hook_lifecycle();
        out.extend(render_evaluated(self.hooks.evaluated));
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
    /// use confit_cli::presentation::summary::{Hooks, Summary};
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::handles::{Route, RouteBase};
    /// use confit_core::plan::Bundle;
    ///
    /// let first = ManifestDocument::new(Route::new(RouteBase::Home, "a").unwrap(), ManifestData::Text { content: "a".into(), mode: None, unmanaged: false});
    /// let second = ManifestDocument::new(Route::new(RouteBase::Home, "b").unwrap(), ManifestData::Text { content: "b".into(), mode: None, unmanaged: false});
    /// let built = Bundle::build(vec![first, second], Vec::new());
    /// let previous = Bundle::empty();
    /// let summary = match built {
    ///     Ok(ref built) => Summary { built, previous: &previous, drift: &[], first_run: false, hooks: Hooks { lifecycle: &[], evaluated: &[] } },
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
        let counts = hook_counts(self.hooks.lifecycle);
        if !counts.is_empty() {
            out.push(format!(
                "Hooks: {} to add, {} to change, {} to destroy.",
                counts.added, counts.changed, counts.destroyed
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
            let sigil = if creates { Sigil::Add } else { Sigil::Update };
            resources.push(painter.paint(
                Sigil::Header,
                &format!("{} {}", sigil.mark(), header_line(document)),
            ));
            if creates {
                for body in entry_bodies(document) {
                    resources.push(
                        painter.paint(Sigil::Add, &format!("  {} {body}", Sigil::Add.mark())),
                    );
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
                .map(|body| painter.paint(Sigil::Add, &format!("  {} {body}", Sigil::Add.mark())))
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
                _ => out.extend(drift_lines(std::slice::from_ref(entry))),
            }
        }
        out
    }

    /// Reads the header line for one steady hunk destination.
    fn drift_hunk_header(&self, path: &Route) -> String {
        let found = self
            .previous
            .manifest
            .documents
            .iter()
            .find(|item| item.destination == *path)
            .or_else(|| {
                self.built
                    .manifest
                    .documents
                    .iter()
                    .find(|item| item.destination == *path)
            });
        match found {
            Some(document) => {
                format!("{} {}", Sigil::Update.mark(), header_line(document))
            }
            None => format!("{} {}", Sigil::Update.mark(), path.display()),
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
                    ManifestData::Tree { members } => format!(
                        "{} {path}: {kind} ({} files)",
                        Sigil::Remove.mark(),
                        members.len()
                    ),
                    _ => format!("{} {path}: {kind}", Sigil::Remove.mark()),
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
            recorded.destination == document.destination
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
    format!(
        "{}: {}",
        document.destination.display(),
        doc_label(document)
    )
}

/// Formats one document header with its lifecycle sigil.
fn status_header(document: &ManifestDocument, status: DocumentStatus) -> String {
    let sigil = match status {
        DocumentStatus::Create => Sigil::Add.mark(),
        DocumentStatus::Update => Sigil::Update.mark(),
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

/// Counts lifecycle entries by change.
///
/// Silent entries never count. The triple describes hook
/// records in the bundle, never execution.
fn hook_counts(lifecycle: &[HookLifecycle]) -> HookCounts {
    let mut counts = HookCounts {
        added: 0,
        changed: 0,
        destroyed: 0,
    };
    for entry in lifecycle {
        if entry.is_added() {
            counts.added += 1;
        } else if entry.is_modified() {
            counts.changed += 1;
        } else if entry.is_removed() {
            counts.destroyed += 1;
        }
    }
    counts
}

/// Collects one document's drift entries for first runs.
///
/// Tree member entries group under their destination path.
/// Every other entry groups under its own path.
fn first_run_entries<'a>(drift: &'a [Drift], document: &ManifestDocument) -> Vec<&'a Drift> {
    let dest = document.destination.display();
    let is_tree = document.data.tree_members().is_some();
    drift
        .iter()
        .filter(|entry| {
            let path = match entry {
                Drift::Key { path, .. }
                | Drift::Hunk { path, .. }
                | Drift::Missing { path }
                | Drift::Unreadable { path, .. } => path.display(),
            };
            if path == dest {
                return true;
            }
            is_tree
                && path
                    .strip_prefix(dest.as_str())
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
        .any(|entry| matches!(entry, Drift::Missing { path } if path == &document.destination))
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
                "  {} tree ({} of {} files changed)",
                Sigil::Update.mark(),
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
                        "  {} {key} = {} -> {}",
                        Sigil::Update.mark(),
                        leaf_text(old_value),
                        leaf_text(new_value)
                    ),
                )),
                (Some(old_value), None) => out.push(painter.paint(
                    Sigil::Remove,
                    &format!(
                        "  {} {key} = {}",
                        Sigil::Remove.mark(),
                        leaf_text(old_value)
                    ),
                )),
                (None, Some(new_value)) => out.push(painter.paint(
                    Sigil::Add,
                    &format!("  {} {key} = {}", Sigil::Add.mark(), leaf_text(new_value)),
                )),
                (None, None) => {}
            },
            Drift::Hunk { hunks, .. } => {
                out.extend(painted_hunk_lines(painter, hunks));
            }
            Drift::Unreadable { reason, .. } => out.push(painter.paint(
                Sigil::Update,
                &format!(
                    "  {} unreadable ({reason}), apply will write desired content",
                    Sigil::Update.mark()
                ),
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
fn member_rel(document: &ManifestDocument, entry: &Drift) -> String {
    let dest = document.destination.display();
    match entry {
        Drift::Key { key, .. } => key.strip_suffix(":mode").unwrap_or(key).to_string(),
        Drift::Hunk { path, .. } | Drift::Missing { path } | Drift::Unreadable { path, .. } => {
            let rendered = path.display();
            rendered
                .strip_prefix(dest.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .unwrap_or(&rendered)
                .to_string()
        }
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
        RcOp::Eval { argv, .. } => format!("eval \"$({})\"", Arg::join(argv)),
        RcOp::Cmd { argv, .. } => Arg::join(argv),
        RcOp::Source { path, .. } => format!("source {}", path.display()),
        RcOp::Env { name, value } => format!("profile {name} = {value}"),
        RcOp::Path { name, dir, .. } => format!("profile {name} = {}", dir.display()),
        RcOp::Alias { name, expansion } => format!("alias {name} = {expansion}"),
    }
}

/// Renders one named rc entry as display text.
fn named_text(op: &RcOp) -> String {
    match op {
        RcOp::Env { name, value } => format!("profile {name} = {value}"),
        RcOp::Path { name, dir, .. } => format!("profile {name} = {}", dir.display()),
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
        ManifestData::Secret { argv, .. } => {
            vec![format!("secret ({})", Arg::join(argv))]
        }
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
fn opaque_ref_label(sha: &Sha, size: u64) -> String {
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
        ) => structured_update_lines(painter, new, old),
        (ManifestData::Link { target: new }, ManifestData::Link { target: old }) => {
            if old == new {
                Vec::new()
            } else {
                vec![painter.paint(
                    Sigil::Update,
                    &format!("  {} target = {old} -> {new}", Sigil::Update.mark()),
                )]
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
                        "  {} content = {} -> {}",
                        Sigil::Update.mark(),
                        opaque_ref_label(old_blob.sha(), *old_size),
                        opaque_ref_label(new_blob.sha(), *new_size)
                    ),
                )]
            }
        }
        (ManifestData::Tree { members: new }, ManifestData::Tree { members: old }) => {
            let changed = confit_core::document::tree_changed(old, new);
            vec![painter.paint(
                Sigil::Update,
                &format!(
                    "  {} tree ({changed} of {} files changed)",
                    Sigil::Update.mark(),
                    new.len()
                ),
            )]
        }
        (ManifestData::Rc(_), ManifestData::Rc(_)) => rc_update_lines(painter, document, recorded),
        _ if touches_opaque(document, recorded) => {
            let mut out = vec![painter.paint(
                Sigil::Update,
                &format!(
                    "  {} kind = {} -> {}",
                    Sigil::Update.mark(),
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
        .map(|body| painter.paint(Sigil::Update, &format!("  {} {body}", Sigil::Update.mark())))
        .collect()
}

/// Builds one recorded-to-desired unified hunk for plan updates.
///
/// File markers never leave this function.
fn recorded_hunk(old: &str, new: &str) -> String {
    diffy::create_patch(old, new)
        .to_string()
        .lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    let old_bytes = match recorded.render(&|route| PathBuf::from(route.display())) {
        Ok(bytes) => bytes,
        Err(_) => return update_fallback(painter, document),
    };
    let new_bytes = match document.render(&|route| PathBuf::from(route.display())) {
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

/// Collects structured leaf changes with old to new values.
///
/// Flattened leaves compare by key, so nested edits read as dotted leaf lines.
fn structured_update_lines(painter: &Painter, new: &Table, old: &Table) -> Vec<String> {
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
                        "  {} {key} = {} -> {}",
                        Sigil::Update.mark(),
                        leaf_text(old_text),
                        leaf_text(new_text)
                    ),
                )),
                (Some(old_text), None) => out.push(painter.paint(
                    Sigil::Remove,
                    &format!("  {} {key} = {}", Sigil::Remove.mark(), leaf_text(old_text)),
                )),
                (None, Some(new_text)) => out.push(painter.paint(
                    Sigil::Add,
                    &format!("  {} {key} = {}", Sigil::Add.mark(), leaf_text(new_text)),
                )),
                (None, None) => {}
            }
        }
    }
    out
}
