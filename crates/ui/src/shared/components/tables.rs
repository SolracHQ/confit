//! Component counts and tables.

use std::{env, path::Path};

use comfy_table::{CellAlignment, ContentArrangement, Table, presets::NOTHING};
use confit_differ::{change::ChangeKind, view::View};
use confit_model::hook;
use extend::ext;

use crate::shared::{
    components::{
        strings::{
            RESTORE_DRIFTED_TITLE, RESTORE_EXACT_ARG, RESTORE_EXACT_TITLE, RESTORE_TITLE,
            RESTORE_UNDO_ARG,
        },
        style::{Outcome, Symbol, strExt as _},
    },
    state::HookLog,
};

/// One document count by write outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocCounts {
    /// The number of added rows.
    pub added: usize,
    /// The number of changed rows.
    pub changed: usize,
    /// The number of removed rows.
    pub removed: usize,
    /// The number of failed rows.
    pub failed: usize,
}

/// One hook count by lifecycle change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HookCounts {
    /// The number of added hooks.
    pub added: usize,
    /// The number of changed hooks.
    pub changed: usize,
    /// The number of destroyed hooks.
    pub destroyed: usize,
}

impl HookCounts {
    /// Returns whether the counts are empty.
    pub fn is_empty(&self) -> bool {
        self.added == 0 && self.changed == 0 && self.destroyed == 0
    }
}

/// One hook count split by execution for a failed run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ErrorHookCounts {
    /// Executed lifecycle buckets.
    pub executed: HookCounts,
    /// Planned hooks with no finished run.
    pub pending: usize,
}

impl ErrorHookCounts {
    /// Returns whether the counts are empty.
    pub fn is_empty(&self) -> bool {
        self.executed.is_empty() && self.pending == 0
    }
}

#[ext(name = KindCounts)]
pub impl<T: AsRef<ChangeKind>> [T] {
    /// Counts rows by change kind.
    fn kind_counts(&self) -> (usize, usize, usize) {
        let mut added = 0;
        let mut changed = 0;
        let mut removed = 0;
        for item in self {
            match item.as_ref() {
                ChangeKind::Addition => added += 1,
                ChangeKind::Change => changed += 1,
                ChangeKind::Removal => removed += 1,
            }
        }
        (added, changed, removed)
    }
}

/// Returns the preview summary table.
pub fn preview_table(view: &View) -> String {
    let (added, changed, removed) = view.rows.kind_counts();
    let docs = DocCounts {
        added,
        changed,
        removed,
        failed: 0,
    };
    results_table(&docs, &hook_counts(view))
}

/// Empty clause text.
const EMPTY_CLAUSE: &str = "[]";

/// One clause mark.
enum Part {
    /// Added rows.
    Add,
    /// Changed rows.
    Update,
    /// Removed rows.
    Remove,
    /// Failed rows.
    Failed,
    /// Pending rows.
    Pending,
}

impl std::fmt::Display for Part {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Part::Add => write!(f, "{}", Symbol::Add),
            Part::Update => write!(f, "{}", Symbol::Update),
            Part::Remove => write!(f, "{}", Symbol::Remove),
            Part::Failed => write!(f, "{}", Outcome::Failed),
            Part::Pending => write!(f, "{}", Outcome::Unattempted),
        }
    }
}

/// Builds the mark parts with only nonzero categories.
fn parts(docs: &DocCounts, pending: usize) -> Vec<(Part, usize)> {
    let mut out = Vec::new();
    if docs.added > 0 {
        out.push((Part::Add, docs.added));
    }
    if docs.changed > 0 {
        out.push((Part::Update, docs.changed));
    }
    if docs.removed > 0 {
        out.push((Part::Remove, docs.removed));
    }
    if docs.failed > 0 {
        out.push((Part::Failed, docs.failed));
    }
    if pending > 0 {
        out.push((Part::Pending, pending));
    }
    out
}

/// Builds one breakdown from mark parts.
fn render_parts(found: &[(Part, usize)]) -> String {
    found
        .iter()
        .map(|(part, count)| format!("{part}: {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Builds one bracket clause from document counts.
pub fn clause(docs: &DocCounts, pending: usize) -> String {
    let found = parts(docs, pending);
    if found.is_empty() {
        return EMPTY_CLAUSE.to_owned();
    }
    format!("[{found}]", found = render_parts(&found))
}

/// Returns the hook lifecycle counts.
pub fn hook_counts(view: &View) -> HookCounts {
    let mut counts = HookCounts::default();
    for entry in hook::diff_lifecycle(&view.wanted.hooks, &view.recorded.hooks) {
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

/// Returns the hook counts split by execution for a failed run.
pub fn error_hook_counts(view: &View, executed: &HookLog) -> ErrorHookCounts {
    let mut remaining: Vec<_> = executed.0.iter().collect();
    let mut out = ErrorHookCounts::default();
    for entry in hook::diff_lifecycle(&view.wanted.hooks, &view.recorded.hooks) {
        let slot = if entry.is_added() {
            Some(&mut out.executed.added)
        } else if entry.is_modified() {
            Some(&mut out.executed.changed)
        } else if entry.is_removed() {
            Some(&mut out.executed.destroyed)
        } else {
            None
        };
        let Some(slot) = slot else {
            continue;
        };
        if let Some(found) = remaining.iter().position(|run| run.argv == entry.hook.argv) {
            remaining.remove(found);
            *slot += 1;
        } else {
            out.pending += 1;
        }
    }
    out
}

/// Documents row label for the closing table.
pub const DOCUMENTS_LABEL: &str = "Documents";

/// Hooks row label for the closing table.
pub const HOOKS_LABEL: &str = "Hooks";

/// Returns the closing Results table.
pub fn results_table(docs: &DocCounts, hooks: &HookCounts) -> String {
    let mut table = Table::new();
    table.load_style(NOTHING);
    table.set_content_arrangement(ContentArrangement::Disabled);
    table.force_no_tty();
    table.set_header(vec![
        String::new(),
        Symbol::Add.to_string(),
        Symbol::Update.to_string(),
        Symbol::Remove.to_string(),
        Outcome::Failed.to_string(),
    ]);
    table.add_row(vec![
        DOCUMENTS_LABEL.to_owned(),
        docs.added.to_string(),
        docs.changed.to_string(),
        docs.removed.to_string(),
        docs.failed.to_string(),
    ]);
    if !hooks.is_empty() {
        table.add_row(vec![
            HOOKS_LABEL.to_owned(),
            hooks.added.to_string(),
            hooks.changed.to_string(),
            hooks.destroyed.to_string(),
            0.to_string(),
        ]);
    }
    for index in 1..=4 {
        if let Some(column) = table.column_mut(index) {
            column.set_cell_alignment(CellAlignment::Center);
        }
    }
    table.to_string()
}

/// Returns the closing restore hints.
pub fn hints(undo: bool, rescued: bool, drifted: bool) -> Vec<String> {
    let binary = bin_name();
    let mut out = Vec::new();
    if rescued {
        out.push(RESTORE_EXACT_TITLE.header());
        out.push(format!(
            "{} {} {}",
            ">".dim(),
            binary.binary(),
            RESTORE_EXACT_ARG.header()
        ));
    }
    if undo {
        if drifted {
            out.push(RESTORE_DRIFTED_TITLE.header());
        } else {
            out.push(RESTORE_TITLE.header());
        }
        out.push(format!(
            "{} {} {}",
            ">".dim(),
            binary.binary(),
            RESTORE_UNDO_ARG.header()
        ));
    }
    out
}

/// Returns the binary name from args or confit if none.
fn bin_name() -> String {
    env::args()
        .next()
        .and_then(|argument| {
            Path::new(&argument)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "confit".to_owned())
}
