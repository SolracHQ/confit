//! Summary
//! Stderr summary over built bundles.

use std::borrow::Cow;
use std::collections::BTreeSet;

use confit_driver::sha::Sha;
use confit_model::arg::Arg;
use confit_model::document::{Data, DocumentKind, RcEntry};
use confit_model::hook::HookLifecycle;
use confit_model::render::inline_bytes;
use confit_model::routes::Route;
use confit_runtime::Difference;
use confit_runtime::manifest::{Document, DocumentStatus, Manifest, summary};

use crate::presentation::difference::{difference_lines, flatten_table, leaf_changes, leaf_text};
use crate::presentation::hooks::lifecycle_lines;

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

/// Difference section title showing manual edits as apply losses.
const DIFFERENCE_TITLE: &str = "Changes outside Confit will be overwritten on next apply";
/// Resources section title over document blocks.
const RESOURCES_TITLE: &str = "Resources";
/// Hooks section title over lifecycle lines.
const HOOKS_TITLE: &str = "Hooks";
/// Summary section title over the closing counts.
const SUMMARY_TITLE: &str = "Summary";

/// Line sigil for paint color.
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
    /// Return the line mark for one sigil.
    ///
    /// Headers carry no lifecycle mark.
    pub fn mark(self) -> char {
        match self {
            Sigil::Add => '+',
            Sigil::Update => '~',
            Sigil::Remove => '-',
            Sigil::Header => ' ',
        }
    }
}

/// One terminal painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Painter {
    /// Color flag for terminal stderr runs.
    color: bool,
}

impl Painter {
    /// Resolve the color decision from the terminal and the environment.
    pub fn new() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none()
                && std::io::IsTerminal::is_terminal(&std::io::stderr()),
        }
    }

    /// Paint one line in the sigil style.
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
    /// Resolve the color decision like `new`.
    fn default() -> Self {
        Self::new()
    }
}

/// Hook display data.
#[derive(Debug)]
pub struct Hooks<'a> {
    /// Lifecycle entries in plan order with removals trailing.
    pub lifecycle: &'a [HookLifecycle<'a>],
}

/// One hook count triple.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookCounts {
    /// Added hook header count.
    pub added: usize,
    /// Changed hook header count.
    pub changed: usize,
    /// Removed hook header count.
    pub destroyed: usize,
}

impl HookCounts {
    /// Report whether one triple is empty of moving hooks.
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.destroyed == 0
    }

    /// Return the total moving hooks.
    pub fn total(&self) -> usize {
        self.added + self.changed + self.destroyed
    }
}

/// One stderr summary.
#[derive(Debug)]
pub struct Summary<'a> {
    /// Built manifest for display.
    pub built: &'a Manifest,
    /// Previous manifest for lifecycle marks.
    pub previous: &'a Manifest,
    /// Manifest versus disk edits leading the text.
    pub differences: &'a [Difference],
    /// First-run flag for absent state.
    pub first_run: bool,
    /// Hook display data for the hooks section.
    pub hooks: Hooks<'a>,
}

impl Summary<'_> {
    /// Render the full stderr summary with color.
    pub fn render(&self) -> String {
        if self.first_run {
            return self.render_first_run();
        }
        let painter = Painter::new();
        let mut lines: Vec<String> = Vec::new();
        push_section(
            &mut lines,
            &painter,
            DIFFERENCE_TITLE,
            self.repeat_difference_lines(&painter),
        );
        push_section(
            &mut lines,
            &painter,
            RESOURCES_TITLE,
            self.resource_lines(&painter),
        );
        push_section(&mut lines, &painter, HOOKS_TITLE, self.hook_lifecycle());
        lines.push(painter.paint(Sigil::Header, SUMMARY_TITLE));
        lines.extend(
            self.summary_lines()
                .iter()
                .map(|line| painter.paint(Sigil::Header, line)),
        );
        lines.join("\n")
    }

    /// Collect resource blocks for repeat runs.
    ///
    /// - Unchanged documents excluded.
    fn resource_lines(&self, painter: &Painter) -> Vec<String> {
        let mut out = Vec::new();
        for document in &self.built.documents {
            let status = document.status(&self.previous.documents);
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

    /// Render hook lifecycle lines from display data.
    fn hook_lifecycle(&self) -> Vec<String> {
        lifecycle_lines(self.hooks.lifecycle)
    }

    /// Render the closing counts lines.
    ///
    /// Hooks line present with moving hooks.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(2);
        if self.first_run {
            let (adds, changes) = self.first_run_counts();
            out.push(format!(
                "Documents: {adds} to add, {changes} to change, 0 to destroy."
            ));
        } else {
            let counts = summary(&self.built.documents, &self.previous.documents);
            out.push(format!(
                "Documents: {} to add, {} to change, {} to destroy.",
                counts.create, counts.update, counts.delete
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

    /// Count first-run creates and overwrites.
    fn first_run_counts(&self) -> (usize, usize) {
        let mut adds = 0;
        let mut changes = 0;
        for document in &self.built.documents {
            let entries = first_run_entries(self.differences, document);
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

    /// Render the first-run form over desired versus disk differences.
    fn render_first_run(&self) -> String {
        let painter = Painter::new();
        let mut lines = Vec::new();
        let mut resources = Vec::new();
        for document in &self.built.documents {
            let entries = first_run_entries(self.differences, document);
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
        push_section(&mut lines, &painter, HOOKS_TITLE, self.hook_lifecycle());
        lines.push(painter.paint(Sigil::Header, SUMMARY_TITLE));
        lines.extend(
            self.summary_lines()
                .iter()
                .map(|line| painter.paint(Sigil::Header, line)),
        );
        lines.join("\n")
    }

    /// Render entry lines for one document under its own status.
    fn document_lines(&self, painter: &Painter, document: &Document) -> Vec<String> {
        match document.status(&self.previous.documents) {
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

    /// Render repeat difference entries with grouped and painted diffs.
    fn repeat_difference_lines(&self, painter: &Painter) -> Vec<String> {
        let mut out = Vec::new();
        for entry in self.differences {
            match entry {
                Difference::Diff { path, diff } => {
                    out.push(painter.paint(Sigil::Header, &self.difference_diff_header(path)));
                    out.extend(painted_diff_lines(painter, diff));
                }
                _ => out.extend(difference_lines(std::slice::from_ref(entry))),
            }
        }
        out
    }

    /// Render the header line for one repeat diff destination.
    fn difference_diff_header(&self, path: &Route) -> String {
        let found = self
            .previous
            .documents
            .iter()
            .find(|item| item.destination == *path)
            .or_else(|| {
                self.built
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

    /// Render delete headers for previous keys missing from the manifest.
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
                    Data::Tree { members } => format!(
                        "{} {path}: {kind} ({} files)",
                        Sigil::Remove.mark(),
                        members.len()
                    ),
                    _ => format!("{} {path}: {kind}", Sigil::Remove.mark()),
                }
            })
            .collect()
    }

    /// Find one recorded document by key with opaque fallback.
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
            recorded.destination == document.destination
                && recorded.key() != document.key()
                && (recorded.is_opaque() || document.is_opaque())
        })
    }
}

/// Render one prepared bundle as its stderr summary.
pub fn render_prepared(prepared: &confit_runtime::Prepared) -> String {
    let lifecycle =
        confit_model::hook::diff_lifecycle(&prepared.desired.hooks, &prepared.previous.hooks);
    Summary {
        built: &prepared.desired,
        previous: &prepared.previous,
        differences: &prepared.differences,
        first_run: prepared.first_run,
        hooks: Hooks {
            lifecycle: &lifecycle,
        },
    }
    .render()
}

/// Return the display label for one document.
fn doc_label(document: &Document) -> Cow<'_, str> {
    match &document.data {
        Data::Structured { format, .. } => Cow::Borrowed(format.name()),
        _ => Cow::Borrowed(document.data.kind().name()),
    }
}

/// Render the header line for one document.
fn header_line(document: &Document) -> String {
    format!(
        "{}: {}",
        document.destination.display(),
        doc_label(document)
    )
}

/// Format one document header with its lifecycle sigil.
fn status_header(document: &Document, status: DocumentStatus) -> String {
    let sigil = match status {
        DocumentStatus::Create => Sigil::Add.mark(),
        DocumentStatus::Update => Sigil::Update.mark(),
        DocumentStatus::Unchanged => ' ',
    };
    format!("{sigil} {}", header_line(document))
}

/// Append one titled section for a body with lines.
fn push_section(lines: &mut Vec<String>, painter: &Painter, title: &str, body: Vec<String>) {
    if body.is_empty() {
        return;
    }
    lines.push(painter.paint(Sigil::Header, title));
    lines.extend(body);
}

/// Count lifecycle entries by change.
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

/// Collect one document's difference entries for first runs.
///
/// - Tree member entries group under their destination path.
/// - Every other entry under its own path.
fn first_run_entries<'a>(
    differences: &'a [Difference],
    document: &Document,
) -> Vec<&'a Difference> {
    let is_tree = document.data.tree_members().is_some();
    differences
        .iter()
        .filter(|entry| {
            let path = match entry {
                Difference::Key { path, .. }
                | Difference::Structured { path, .. }
                | Difference::Diff { path, .. }
                | Difference::Missing { path }
                | Difference::Unreadable { path, .. } => path,
            };
            path == &document.destination || is_tree && member_of(&document.destination, path)
        })
        .collect()
}

/// Report whether one tree member route nests under its destination.
fn member_of(destination: &Route, path: &Route) -> bool {
    path.base() == destination.base()
        && path.relative() != destination.relative()
        && path.relative().starts_with(destination.relative())
}

/// Report whether one first-run group is whole.
///
/// - Whole-file missing entries always land whole.
/// - Whole trees with missing entries alone land whole.
fn first_run_creates(document: &Document, entries: &[&Difference]) -> bool {
    if entries
        .iter()
        .any(|entry| matches!(entry, Difference::Missing { path } if path == &document.destination))
    {
        return true;
    }
    if document.data.tree_members().is_some() {
        return entries
            .iter()
            .all(|entry| matches!(entry, Difference::Missing { .. }));
    }
    false
}

/// Render one first-run update group with disk values first.
///
/// Trees collapse to one changed member count. Unreadable paths show the
/// replacement.
fn first_run_updates(
    painter: &Painter,
    document: &Document,
    entries: &[&Difference],
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
            Difference::Key { key, old, new, .. } => {
                out.extend(paint_key_line(painter, key, old, new));
            }
            Difference::Structured { old, new, .. } => {
                for change in leaf_changes(old, new) {
                    out.extend(paint_key_line(
                        painter,
                        &change.key,
                        &change.old,
                        &change.new,
                    ));
                }
            }
            Difference::Diff { diff, .. } => {
                out.extend(painted_diff_lines(painter, diff));
            }
            Difference::Unreadable { reason, .. } => out.push(painter.paint(
                Sigil::Update,
                &format!(
                    "  {} unreadable ({reason}), apply will write desired content",
                    Sigil::Update.mark()
                ),
            )),
            Difference::Missing { .. } => {}
        }
    }
    out
}

/// Paint unified diff content lines by their leading symbol.
fn painted_diff_lines(painter: &Painter, diff: &str) -> Vec<String> {
    diff.lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .map(|line| paint_diff_line(painter, line))
        .collect()
}

/// Paint one diff content line by its leading symbol.
fn paint_diff_line(painter: &Painter, line: &str) -> String {
    if let Some(rest) = line.strip_prefix('+') {
        painter.paint(Sigil::Add, &format!("  +{rest}"))
    } else if let Some(rest) = line.strip_prefix('-') {
        painter.paint(Sigil::Remove, &format!("  -{rest}"))
    } else {
        format!("  {}", line.strip_prefix(' ').unwrap_or(line))
    }
}

/// Render one entry's tree member rel under its destination.
fn member_rel(document: &Document, entry: &Difference) -> String {
    match entry {
        Difference::Key { key, .. } => key.strip_suffix(":mode").unwrap_or(key).to_string(),
        Difference::Structured { path, .. }
        | Difference::Diff { path, .. }
        | Difference::Missing { path }
        | Difference::Unreadable { path, .. } => path
            .relative()
            .strip_prefix(document.destination.relative())
            .map_or_else(|_| path.display(), |rel| rel.display().to_string()),
    }
}

/// Render one exec payload as shell text.
fn init_text(entry: &RcEntry) -> String {
    match entry {
        RcEntry::Eval { argv, .. } => format!("eval \"$({})\"", Arg::join(argv)),
        RcEntry::Cmd { argv, .. } => Arg::join(argv),
        RcEntry::Source { path, .. } => format!("source {}", path.display()),
        RcEntry::Env { name, value, .. } => format!("profile {name} = {value}"),
        RcEntry::Path { name, dir, .. } => {
            format!("profile {name} = {}", dir.display())
        }
        RcEntry::Alias {
            name, expansion, ..
        } => format!("alias {name} = {expansion}"),
    }
}

/// Render one named rc entry as display text.
fn named_text(entry: &RcEntry) -> String {
    match entry {
        RcEntry::Env { name, value, .. } => format!("profile {name} = {value}"),
        RcEntry::Path { name, dir, .. } => {
            format!("profile {name} = {}", dir.display())
        }
        RcEntry::Alias {
            name, expansion, ..
        } => format!("alias {name} = {expansion}"),
        _ => init_text(entry),
    }
}

/// Collect plain entry bodies for one document.
fn entry_bodies(document: &Document) -> Vec<String> {
    match &document.data {
        Data::Structured { data, .. } => flatten_table(data)
            .into_iter()
            .map(|(key, value)| format!("{key} = {}", leaf_text(&value)))
            .collect(),
        Data::Text { content, .. } => {
            if content.is_empty() {
                Vec::new()
            } else {
                content.split('\n').map(str::to_string).collect()
            }
        }
        Data::Link { target } => vec![target.display()],
        Data::Opaque { handle: blob, .. } => {
            vec![format!("opaque {}", opaque_ref_label(blob.sha256()))]
        }
        Data::Tree { members } => vec![format!("tree ({} files)", members.len())],
        Data::Rc(rc) => {
            let mut out = Vec::new();
            for entry in rc.profile.iter().chain(rc.config.iter()) {
                match entry {
                    RcEntry::Env { .. } | RcEntry::Path { .. } | RcEntry::Alias { .. } => {
                        out.push(named_text(entry));
                    }
                    _ => out.push(init_text(entry)),
                }
            }
            for (index, entry) in rc.final_entries.iter().enumerate() {
                match entry {
                    RcEntry::Env { .. } | RcEntry::Path { .. } | RcEntry::Alias { .. } => {
                        out.push(named_text(entry));
                    }
                    _ => out.push(format!("init[{index}] = {}", init_text(entry))),
                }
            }
            out
        }
    }
}

/// Report whether the opaque kind is on either side.
fn touches_opaque(first: &Document, second: &Document) -> bool {
    matches!(first.data.kind(), DocumentKind::Opaque)
        || matches!(second.data.kind(), DocumentKind::Opaque)
}

/// Render the hash label for one opaque ref.
fn opaque_ref_label(sha: &Sha) -> String {
    format!("sha256:{sha}")
}

/// Collect update lines with old to new values.
fn update_lines(painter: &Painter, document: &Document, recorded: &Document) -> Vec<String> {
    match (&document.data, &recorded.data) {
        (Data::Structured { data: new, .. }, Data::Structured { data: old, .. }) => {
            structured_update_lines(painter, new, old)
        }
        (Data::Link { target: new }, Data::Link { target: old }) => {
            if old == new {
                Vec::new()
            } else {
                vec![painter.paint(
                    Sigil::Update,
                    &format!(
                        "  {} target = {} -> {}",
                        Sigil::Update.mark(),
                        old.display(),
                        new.display()
                    ),
                )]
            }
        }
        (
            Data::Opaque {
                handle: new_blob, ..
            },
            Data::Opaque {
                handle: old_blob, ..
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
                        opaque_ref_label(old_blob.sha256()),
                        opaque_ref_label(new_blob.sha256())
                    ),
                )]
            }
        }
        (Data::Tree { members: new }, Data::Tree { members: old }) => {
            let changed = confit_model::document::tree_changed(old, new);
            vec![painter.paint(
                Sigil::Update,
                &format!(
                    "  {} tree ({changed} of {} files changed)",
                    Sigil::Update.mark(),
                    new.len()
                ),
            )]
        }
        (Data::Rc(_), Data::Rc(_)) => rc_update_lines(painter, document, recorded),
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

/// Render desired entry bodies under the update sigil.
fn update_fallback(painter: &Painter, document: &Document) -> Vec<String> {
    entry_bodies(document)
        .into_iter()
        .map(|body| painter.paint(Sigil::Update, &format!("  {} {body}", Sigil::Update.mark())))
        .collect()
}

/// Build one recorded-to-desired unified diff for plan updates.
fn recorded_diff(old: &str, new: &str) -> String {
    diffy::create_patch(old, new)
        .to_string()
        .lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render one rc update as a recorded to desired text diff with desired-body
/// fallback.
fn rc_update_lines(painter: &Painter, document: &Document, recorded: &Document) -> Vec<String> {
    let old_bytes = match inline_bytes(&recorded.data) {
        Ok(bytes) => bytes,
        Err(_) => return update_fallback(painter, document),
    };
    let new_bytes = match inline_bytes(&document.data) {
        Ok(bytes) => bytes,
        Err(_) => return update_fallback(painter, document),
    };
    if old_bytes == new_bytes {
        return Vec::new();
    }
    let old = String::from_utf8_lossy(&old_bytes);
    let new = String::from_utf8_lossy(&new_bytes);
    let diff = recorded_diff(&old, &new);
    painted_diff_lines(painter, &diff)
}

/// Split a manifest key into kind and path halves.
fn split_key(key: &str) -> (&str, &str) {
    match key.find(':') {
        Some(index) => (&key[..index], &key[index + 1..]),
        None => ("", key),
    }
}

/// Collect structured leaf changes with old to new values.
fn structured_update_lines(
    painter: &Painter,
    new: &serde_json::Value,
    old: &serde_json::Value,
) -> Vec<String> {
    let mut out = Vec::new();
    for change in leaf_changes(old, new) {
        out.extend(paint_key_line(
            painter,
            &change.key,
            &change.old,
            &change.new,
        ));
    }
    out
}

/// Paint one leaf change line for one dotted key.
fn paint_key_line(
    painter: &Painter,
    key: &str,
    old: &Option<serde_json::Value>,
    new: &Option<serde_json::Value>,
) -> Option<String> {
    match (old, new) {
        (Some(old_value), Some(new_value)) => Some(painter.paint(
            Sigil::Update,
            &format!(
                "  {} {key} = {} -> {}",
                Sigil::Update.mark(),
                leaf_text(old_value),
                leaf_text(new_value)
            ),
        )),
        (Some(old_value), None) => Some(painter.paint(
            Sigil::Remove,
            &format!(
                "  {} {key} = {}",
                Sigil::Remove.mark(),
                leaf_text(old_value)
            ),
        )),
        (None, Some(new_value)) => Some(painter.paint(
            Sigil::Add,
            &format!("  {} {key} = {}", Sigil::Add.mark(), leaf_text(new_value)),
        )),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use confit_model::arg::Arg;
    use confit_model::condition::Condition;
    use confit_model::hook::Hook;
    use confit_runtime::manifest::Manifest;

    fn probe_hook() -> Hook {
        Hook {
            argv: vec![Arg::Text("absent-probe".into()), Arg::Text("run".into())],
            path: Vec::new(),
            id: None,
            requires: Condition::InPath {
                name: "absent-probe".into(),
            },
            when: Condition::default(),
            check: Condition::default(),
            timeout_secs: 60,
        }
    }

    #[test]
    fn tree_first_run_groups_members_under_destination() {
        use confit_model::document::Payload;
        use confit_model::routes::RouteBase;
        use confit_store::handles::BlobRef;
        let dest = Route::new(RouteBase::Data, "fonts/JetBrainsMono").unwrap();
        let blob = BlobRef::new(Sha::hash(b"x"), Sha::hash(b"y"));
        let mut members = BTreeMap::new();
        members.insert(
            "a.ttf".to_owned(),
            confit_model::document::TreeMember::Full {
                handle: Payload::Resolved(blob),
                mode: 0o644,
            },
        );
        let document = Document {
            destination: dest.clone(),
            data: Data::Tree { members },
            data_hash: String::new(),
        };
        let member_path = dest.join("a.ttf");
        let differences = vec![confit_runtime::Difference::Missing { path: member_path }];
        let entries = first_run_entries(&differences, &document);
        assert_eq!(entries.len(), 1, "tree members group under destinations");
        assert_eq!(
            member_rel(&document, entries[0]),
            "a.ttf",
            "member lines show the relative path"
        );
    }

    #[test]
    fn preview_renders_missing_binaries_without_resolving() {
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = confit_store::Stores::new(confit_store::StoreRoots::default(), sender);
        let desired = Manifest::build(&stores, BTreeMap::new(), vec![probe_hook()]).unwrap();
        let prepared = confit_runtime::Prepared {
            desired,
            previous: Manifest::empty(),
            differences: Vec::new(),
            first_run: false,
            changed: BTreeSet::new(),
        };
        let text = render_prepared(&prepared);
        assert!(
            text.contains("+ absent-probe run"),
            "preview holds the hook line"
        );
        assert!(
            text.contains("requires (in_path(absent-probe))"),
            "preview holds condition text"
        );
        assert!(text.contains("Hooks: 1 to add"), "preview counts the hook");
    }
}
