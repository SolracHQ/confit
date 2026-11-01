//! Renders the terminal summary over prepared manifests.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use confit_core::sha::Sha;
use confit_model::arg::Arg;
use confit_model::document::{self as document_mod, Data, Document, RcEntry};
use confit_model::hook::{self, HookLifecycle};
use confit_model::routes::Route;
use confit_runtime::manifest::Manifest;
use confit_runtime::manifest::status::{self, DocumentStatus};
use confit_runtime::{Difference, Prepared};
use confit_store::handles::ResourceHandle;

use crate::presentation::difference;
use crate::presentation::hooks;

/// The escape sequence that paints a changed line yellow.
const UPDATE_STYLE: &str = "\x1b[33m";
/// The escape sequence that paints an added line green.
const ADD_STYLE: &str = "\x1b[32m";
/// The escape sequence that paints a removed line red.
const REMOVE_STYLE: &str = "\x1b[31m";
/// The escape sequence that paints a header bold.
const HEADER_STYLE: &str = "\x1b[1m";
/// The escape sequence that clears the style.
const RESET: &str = "\x1b[0m";

/// The differences title that shows manual edits as apply losses.
const DIFFERENCE_TITLE: &str = "Changes outside Confit will be overwritten on next apply";
/// The resources title over document blocks.
const RESOURCES_TITLE: &str = "Resources";
/// The hooks title over lifecycle lines.
const HOOKS_TITLE: &str = "Hooks";
/// The summary title over the closing counts.
const SUMMARY_TITLE: &str = "Summary";

/// The kind of display line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sigil {
    /// An added line with the plus mark.
    Add,
    /// A changed line with the tilde mark.
    Update,
    /// A removed line with the minus mark.
    Remove,
    /// A section title with the space mark.
    Header,
}

impl Sigil {
    /// Returns the character that starts the line.
    pub fn mark(self) -> char {
        match self {
            Sigil::Add => '+',
            Sigil::Update => '~',
            Sigil::Remove => '-',
            Sigil::Header => ' ',
        }
    }
}

/// The terminal line painter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Painter {
    /// The flag that turns on the color escapes.
    color: bool,
}

impl Painter {
    /// Resolves the color decision from the environment and the terminal.
    pub fn new() -> Self {
        Self {
            color: std::env::var_os("NO_COLOR").is_none()
                && std::io::IsTerminal::is_terminal(&std::io::stderr()),
        }
    }

    /// Paints one line in the style of its sigil.
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
    /// Resolves the color decision for the default painter.
    fn default() -> Self {
        Self::new()
    }
}

/// One block of hook lines.
#[derive(Debug)]
pub struct Hooks<'a> {
    /// The lifecycle entries in plan order with removals last.
    pub lifecycle: &'a [HookLifecycle<'a>],
}

/// The hook line counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookCounts {
    /// The number of added hook lines.
    pub added: usize,
    /// The number of changed hook lines.
    pub changed: usize,
    /// The number of removed hook lines.
    pub destroyed: usize,
}

impl HookCounts {
    /// Returns whether every hook count is zero.
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.destroyed == 0
    }

    /// Returns the number of hooks that move.
    pub fn total(&self) -> usize {
        self.added + self.changed + self.destroyed
    }
}

/// One bundle summary.
#[derive(Debug)]
pub struct Summary<'a> {
    /// The manifest that the summary shows.
    pub built: &'a Manifest,
    /// The recorded manifest that supplies the lifecycle marks.
    pub previous: &'a Manifest,
    /// The manifest versus disk differences that lead the text.
    pub differences: &'a [Difference],
    /// The flag that marks a run with no recorded manifest.
    pub first_run: bool,
    /// The hook lines for the hooks section.
    pub hooks: Hooks<'a>,
}

impl Summary<'_> {
    /// Renders the summary for stderr and picks the first-run form.
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

    /// Collects the resource blocks that a repeat run shows.
    ///
    /// - An unchanged document adds no block.
    fn resource_lines(&self, painter: &Painter) -> Vec<String> {
        let mut out = Vec::new();
        for document in &self.built.documents {
            let status = status::status(
                document,
                &self.previous.documents,
                &self.built.hashes,
                &self.previous.hashes,
            );
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

    /// Renders the hook lifecycle lines from the recorded hooks.
    fn hook_lifecycle(&self) -> Vec<String> {
        hooks::lifecycle_lines(self.hooks.lifecycle)
    }

    /// Renders the closing count lines.
    ///
    /// - The hooks line appears when a hook moves.
    pub fn summary_lines(&self) -> Vec<String> {
        let mut out = Vec::with_capacity(2);
        if self.first_run {
            let (adds, changes) = self.first_run_counts();
            out.push(format!(
                "Documents: {adds} to add, {changes} to change, 0 to destroy."
            ));
        } else {
            let counts = status::summary(
                &self.built.documents,
                &self.built.hashes,
                &self.previous.documents,
                &self.previous.hashes,
            );
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

    /// Counts the documents that a first run creates or overwrites.
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

    /// Renders the first-run form against the disk differences.
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

    /// Renders the entry lines that one document's status calls for.
    fn document_lines(&self, painter: &Painter, document: &Document) -> Vec<String> {
        match status::status(
            document,
            &self.previous.documents,
            &self.built.hashes,
            &self.previous.hashes,
        ) {
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

    /// Renders the difference entries of a repeat run in painted groups.
    fn repeat_difference_lines(&self, painter: &Painter) -> Vec<String> {
        let mut out = Vec::new();
        for entry in self.differences {
            match entry {
                Difference::Diff { path, diff } => {
                    out.push(painter.paint(Sigil::Header, &self.difference_diff_header(path)));
                    out.extend(painted_diff_lines(painter, diff));
                }
                _ => out.extend(difference::difference_lines(std::slice::from_ref(entry))),
            }
        }
        out
    }

    /// Renders the header line for one changed destination.
    fn difference_diff_header(&self, path: &Route) -> String {
        let found = self
            .previous
            .documents
            .iter()
            .find(|item| item.path == *path)
            .or_else(|| self.built.documents.iter().find(|item| item.path == *path));
        match found {
            Some(document) => {
                format!("{} {}", Sigil::Update.mark(), header_line(document))
            }
            None => format!("{} {}", Sigil::Update.mark(), path.display()),
        }
    }

    /// Renders the delete headers for previous keys that the manifest drops.
    fn delete_headers(&self) -> Vec<String> {
        let seen: BTreeSet<String> = self.built.documents.iter().map(status::key).collect();
        self.previous
            .documents
            .iter()
            .filter(|recorded| {
                !seen.contains(&status::key(recorded))
                    && !status::superseded_by(recorded, &self.built.documents)
            })
            .map(|recorded| {
                let key = status::key(recorded);
                let (kind, path) = split_key(&key);
                match &recorded.data {
                    Data::Tree(found) => format!(
                        "{} {path}: {kind} ({} files)",
                        Sigil::Remove.mark(),
                        found.members.len()
                    ),
                    _ => format!("{} {path}: {kind}", Sigil::Remove.mark()),
                }
            })
            .collect()
    }

    /// Finds the matching recorded document, else the opaque one.
    fn find_recorded<'a>(&'a self, document: &Document) -> Option<&'a Document> {
        if let Some(found) = self
            .previous
            .documents
            .iter()
            .find(|item| status::key(item) == status::key(document))
        {
            return Some(found);
        }
        status::find_opaque_sibling(&self.previous.documents, document)
    }
}

/// Renders the stderr summary for one prepared bundle.
pub fn render_prepared(prepared: &Prepared) -> String {
    let lifecycle = hook::diff_lifecycle(&prepared.desired.hooks, &prepared.previous.hooks);
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

/// Returns the label that a document line shows.
fn doc_label(document: &Document) -> Cow<'_, str> {
    match &document.data {
        Data::Structured(found) => Cow::Borrowed(found.format.name()),
        _ => Cow::Borrowed(kind_name(&document.data)),
    }
}

/// Returns the name of the payload kind.
fn kind_name(data: &Data) -> &'static str {
    match data {
        Data::Structured(_) => "structured",
        Data::Text(_) => "text",
        Data::Link(_) => "link",
        Data::Rc(_) => "rc",
        Data::Opaque(_) => "opaque",
        Data::Tree(_) => "tree",
    }
}

/// Renders the path and the label of one document.
fn header_line(document: &Document) -> String {
    format!("{}: {}", document.path.display(), doc_label(document))
}

/// Formats one document header with the sigil of its status.
fn status_header(document: &Document, status: DocumentStatus) -> String {
    let sigil = match status {
        DocumentStatus::Create => Sigil::Add.mark(),
        DocumentStatus::Update => Sigil::Update.mark(),
        DocumentStatus::Unchanged => ' ',
    };
    format!("{sigil} {}", header_line(document))
}

/// Appends one titled section and skips an empty body.
fn push_section(lines: &mut Vec<String>, painter: &Painter, title: &str, body: Vec<String>) {
    if body.is_empty() {
        return;
    }
    lines.push(painter.paint(Sigil::Header, title));
    lines.extend(body);
}

/// Counts the lifecycle entries for each change kind.
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

/// Collects one document's difference entries for a first run.
fn first_run_entries<'a>(
    differences: &'a [Difference],
    document: &Document,
) -> Vec<&'a Difference> {
    let is_tree = matches!(document.data, Data::Tree(_));
    differences
        .iter()
        .filter(|entry| {
            let path = match entry {
                Difference::Key { path, .. }
                | Difference::Structured { path, .. }
                | Difference::Diff { path, .. }
                | Difference::Missing { path }
                | Difference::Blocked { path }
                | Difference::Unreadable { path, .. } => path,
            };
            path == &document.path || is_tree && member_of(&document.path, path)
        })
        .collect()
}

/// Returns whether a route sits under its destination path.
fn member_of(destination: &Route, path: &Route) -> bool {
    path.base() == destination.base()
        && path.relative_path() != destination.relative_path()
        && path
            .relative_path()
            .starts_with(destination.relative_path())
}

/// Returns whether every entry in the group is a create.
fn first_run_creates(document: &Document, entries: &[&Difference]) -> bool {
    if entries
        .iter()
        .any(|entry| matches!(entry, Difference::Missing { path } if path == &document.path))
    {
        return true;
    }
    if matches!(document.data, Data::Tree(_)) {
        return entries
            .iter()
            .all(|entry| matches!(entry, Difference::Missing { .. }));
    }
    false
}

/// Renders a first-run group as an update with the disk values first.
fn first_run_updates(
    painter: &Painter,
    document: &Document,
    entries: &[&Difference],
) -> Vec<String> {
    if let Data::Tree(found) = &document.data {
        let members = &found.members;
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
                for change in difference::leaf_changes(old, new) {
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
            Difference::Blocked { .. } => out.push(painter.paint(
                Sigil::Update,
                &format!(
                    "  {} blocked (directory blocks the path), remove the directory or apply fails",
                    Sigil::Update.mark()
                ),
            )),
            Difference::Missing { .. } => {}
        }
    }
    out
}

/// Paints the diff lines that carry a content symbol.
fn painted_diff_lines(painter: &Painter, diff: &str) -> Vec<String> {
    diff.lines()
        .filter(|line| {
            !(line.starts_with("---") || line.starts_with("+++") || line.starts_with("@@"))
        })
        .map(|line| paint_diff_line(painter, line))
        .collect()
}

/// Paints one diff line by its first character.
fn paint_diff_line(painter: &Painter, line: &str) -> String {
    if let Some(rest) = line.strip_prefix('+') {
        painter.paint(Sigil::Add, &format!("  +{rest}"))
    } else if let Some(rest) = line.strip_prefix('-') {
        painter.paint(Sigil::Remove, &format!("  -{rest}"))
    } else {
        format!("  {}", line.strip_prefix(' ').unwrap_or(line))
    }
}

/// Renders one entry's tree member path under its destination.
fn member_rel(document: &Document, entry: &Difference) -> String {
    match entry {
        Difference::Key { key, .. } => key.strip_suffix(":mode").unwrap_or(key).to_string(),
        Difference::Structured { path, .. }
        | Difference::Diff { path, .. }
        | Difference::Missing { path }
        | Difference::Blocked { path }
        | Difference::Unreadable { path, .. } => path
            .relative_path()
            .strip_prefix(document.path.relative_path())
            .map_or_else(|_| path.display(), |rel| rel.display().to_string()),
    }
}

/// Renders one exec entry as shell text.
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

/// Renders one named rc entry as a shell line.
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

/// Collects the plain entry lines for one document.
fn entry_bodies(document: &Document) -> Vec<String> {
    match &document.data {
        Data::Structured(found) => difference::flatten_table(&found.data)
            .into_iter()
            .map(|(key, value)| format!("{key} = {}", difference::leaf_text(&value)))
            .collect(),
        Data::Text(found) => {
            if found.content.is_empty() {
                Vec::new()
            } else {
                found.content.split('\n').map(str::to_string).collect()
            }
        }
        Data::Link(found) => vec![found.target.display()],
        Data::Opaque(found) => {
            vec![format!("opaque {}", opaque_handle_label(&found.handle))]
        }
        Data::Tree(found) => vec![format!("tree ({} files)", found.members.len())],
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
            for (index, entry) in rc.r#final.iter().enumerate() {
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

/// Returns whether either document is opaque.
fn touches_opaque(first: &Document, second: &Document) -> bool {
    matches!(first.data, Data::Opaque(_)) || matches!(second.data, Data::Opaque(_))
}

/// Renders the label for one cache reference.
fn opaque_ref_label(sha: &Sha) -> String {
    format!("sha256:{sha}")
}

/// Renders the label for one handle's source.
fn opaque_handle_label(handle: &ResourceHandle) -> String {
    match handle {
        ResourceHandle::Cache(sha) => opaque_ref_label(sha),
        ResourceHandle::Project(path, _) => path.display().to_string(),
    }
}

/// Collects the update lines between the recorded and the desired document.
fn update_lines(painter: &Painter, document: &Document, recorded: &Document) -> Vec<String> {
    match (&document.data, &recorded.data) {
        (Data::Structured(new), Data::Structured(old)) => {
            structured_update_lines(painter, &new.data, &old.data)
        }
        (Data::Link(new), Data::Link(old)) => {
            if old.target == new.target {
                Vec::new()
            } else {
                vec![painter.paint(
                    Sigil::Update,
                    &format!(
                        "  {} target = {} -> {}",
                        Sigil::Update.mark(),
                        old.target.display(),
                        new.target.display()
                    ),
                )]
            }
        }
        (Data::Opaque(new), Data::Opaque(old)) => {
            if old.handle == new.handle {
                Vec::new()
            } else {
                vec![painter.paint(
                    Sigil::Update,
                    &format!(
                        "  {} content = {} -> {}",
                        Sigil::Update.mark(),
                        opaque_handle_label(&old.handle),
                        opaque_handle_label(&new.handle)
                    ),
                )]
            }
        }
        (Data::Tree(new), Data::Tree(old)) => {
            let changed = document_mod::tree_changed(&old.members, &new.members);
            vec![painter.paint(
                Sigil::Update,
                &format!(
                    "  {} tree ({changed} of {} files changed)",
                    Sigil::Update.mark(),
                    new.members.len()
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
                    kind_name(&recorded.data),
                    kind_name(&document.data)
                ),
            )];
            out.extend(update_fallback(painter, document));
            out
        }
        _ => update_fallback(painter, document),
    }
}

/// Renders the desired bodies when no recorded body differs.
fn update_fallback(painter: &Painter, document: &Document) -> Vec<String> {
    entry_bodies(document)
        .into_iter()
        .map(|body| painter.paint(Sigil::Update, &format!("  {} {body}", Sigil::Update.mark())))
        .collect()
}

/// Builds the unified diff from the recorded text to the desired text.
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

/// Renders one rc update as a text diff from recorded to desired.
fn rc_update_lines(painter: &Painter, document: &Document, recorded: &Document) -> Vec<String> {
    let old_bytes = match confit_runtime::render_document(recorded) {
        Ok(bytes) => bytes,
        Err(_) => return update_fallback(painter, document),
    };
    let new_bytes = match confit_runtime::render_document(document) {
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

/// Splits a manifest key into the kind and the path.
fn split_key(key: &str) -> (&str, &str) {
    match key.find(':') {
        Some(index) => (&key[..index], &key[index + 1..]),
        None => ("", key),
    }
}

/// Collects the changed leaves between the old and the new map.
fn structured_update_lines(
    painter: &Painter,
    new: &BTreeMap<String, serde_json::Value>,
    old: &BTreeMap<String, serde_json::Value>,
) -> Vec<String> {
    let mut out = Vec::new();
    for change in difference::leaf_changes(old, new) {
        out.extend(paint_key_line(
            painter,
            &change.key,
            &change.old,
            &change.new,
        ));
    }
    out
}

/// Paints one line for one changed leaf under a dotted key.
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
                difference::leaf_text(old_value),
                difference::leaf_text(new_value)
            ),
        )),
        (Some(old_value), None) => Some(painter.paint(
            Sigil::Remove,
            &format!(
                "  {} {key} = {}",
                Sigil::Remove.mark(),
                difference::leaf_text(old_value)
            ),
        )),
        (None, Some(new_value)) => Some(painter.paint(
            Sigil::Add,
            &format!(
                "  {} {key} = {}",
                Sigil::Add.mark(),
                difference::leaf_text(new_value)
            ),
        )),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use confit_core::version::VERSION;
    use confit_model::arg::Arg;
    use confit_model::condition::Condition;
    use confit_model::document::{FileMode, TreeDocument, TreeMember};
    use confit_model::hook::{Hook, HookTimeout};
    use confit_model::id::Id;
    use confit_runtime::manifest::Manifest;
    use confit_runtime::prepare;

    fn probe_hook() -> Hook {
        Hook {
            argv: vec![Arg::Text("absent-probe".into()), Arg::Text("run".into())],
            path: Vec::new(),
            id: Id::of("absent-probe"),
            requires: Condition::InPath {
                name: "absent-probe".into(),
            },
            when: Condition::default(),
            check: Condition::default(),
            timeout_secs: HookTimeout::new(60),
        }
    }

    #[test]
    fn tree_first_run_groups_members_under_destination() {
        use confit_model::routes::RouteBase;
        use confit_store::handles::ResourceHandle;
        let dest = Route::new(RouteBase::Data, "fonts/JetBrainsMono").unwrap();
        let blob = Sha::hash(b"x");
        let mut members = BTreeMap::new();
        members.insert(
            "a.ttf".to_owned(),
            TreeMember::Full {
                handle: ResourceHandle::Cache(blob),
                mode: FileMode::new(0o644),
            },
        );
        let document = Document {
            path: dest.clone(),
            data: Data::Tree(TreeDocument { members }),
        };
        let member_path = dest.join("a.ttf");
        let differences = vec![Difference::Missing { path: member_path }];
        let entries = first_run_entries(&differences, &document);
        assert_eq!(
            entries.len(),
            1,
            "the tree members group under the destination"
        );
        assert_eq!(
            member_rel(&document, entries[0]),
            "a.ttf",
            "the member line shows the relative path"
        );
    }

    #[test]
    fn preview_renders_an_absent_binary_hook() {
        let desired = prepare::build(Vec::new(), vec![probe_hook()]).unwrap();
        let prepared = Prepared {
            desired,
            previous: Manifest {
                version: VERSION,
                documents: Vec::new(),
                hashes: BTreeMap::new(),
                hooks: Vec::new(),
            },
            differences: Vec::new(),
            warnings: Vec::new(),
            first_run: false,
            changed: BTreeSet::new(),
        };
        let text = render_prepared(&prepared);
        assert!(
            text.contains("+ absent-probe run"),
            "the preview holds the hook line"
        );
        assert!(
            text.contains("requires (in_path(absent-probe))"),
            "the preview holds the condition text"
        );
        assert!(
            text.contains("Hooks: 1 to add"),
            "the preview counts the hook"
        );
    }
}
