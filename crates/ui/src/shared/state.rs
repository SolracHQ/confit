//! Shared state and text helpers.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use confit_core::error::Error;
use confit_differ::{change::Change, view::View};
use confit_events::protocol::{Applied, ApplyOutcome, TaskKind, TaskNote};
use confit_model::arg::Arg;

use crate::shared::components::{
    strings::CACHED,
    style::{Outcome, strExt as _},
    tables::DocCounts,
};

/// Evaluation phase title.
pub const EVALUATION: &str = "Evaluation";
/// Packaging phase title.
pub const PACKAGING: &str = "Packaging";
/// Persisting phase title.
pub const PERSISTING: &str = "Persisting";
/// Applying-changes phase title.
pub const APPLYING_CHANGES: &str = "Applying Changes";
/// Pool fill progress label.
const POOL_FILL: &str = "compressing pool entries";
/// Pool prune progress label.
const POOL_PRUNE: &str = "pruning pool entries";
/// Artifact collection prefix.
const COLLECTING: &str = "collecting artifacts";
/// Bundle packing prefix.
const PACKING: &str = "packing";

/// Write outcomes by expanded host path.
#[derive(Debug, Clone, Default)]
pub struct OutcomeMap(BTreeMap<PathBuf, ApplyOutcome>);

impl OutcomeMap {
    /// Builds an empty outcome map.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Records one write outcome.
    pub fn record(&mut self, expanded: PathBuf, outcome: ApplyOutcome) {
        self.0.insert(expanded, outcome);
    }

    /// Builds the outcome map.
    pub fn from_applied(applied: &Applied) -> Self {
        let mut out = Self::new();
        for change in &applied.changes {
            out.record(PathBuf::from(&change.path), change.outcome);
        }
        out
    }

    /// Returns the outcome.
    pub fn get(&self, expanded: &Path) -> Option<ApplyOutcome> {
        self.0.get(expanded).copied()
    }

    /// Returns whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the document counts by write outcome.
    pub fn doc_counts(&self) -> DocCounts {
        let (added, changed, removed, failed) = ApplyOutcome::count(self.0.values().copied());
        DocCounts {
            added,
            changed,
            removed,
            failed,
        }
    }

    /// Returns the write state.
    pub fn outcome_for(&self, change: &Change) -> Outcome {
        match self.get(&change.expanded) {
            Some(ApplyOutcome::Failed) => Outcome::Failed,
            Some(_) => Outcome::Applied,
            None => Outcome::Unattempted,
        }
    }

    /// Returns the combined write state.
    pub fn group_outcome(&self, members: &[&Change]) -> Outcome {
        let states: Vec<Outcome> = members
            .iter()
            .map(|found| self.outcome_for(found))
            .collect();
        if states.is_empty() {
            return Outcome::Unattempted;
        }
        if states.contains(&Outcome::Failed) {
            return Outcome::Failed;
        }
        if states.iter().all(|found| *found == Outcome::Applied) {
            return Outcome::Applied;
        }
        Outcome::Unattempted
    }

    /// Returns the count of changes with no outcome.
    pub fn pending(&self, view: &View) -> usize {
        view.iter()
            .filter(|change| self.get(&change.expanded).is_none())
            .count()
    }
}

/// One finished hook execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    /// Hook arguments.
    pub argv: Vec<Arg>,
    /// Spawned command lines.
    pub spawned: Vec<String>,
}

/// Finished hook executions.
#[derive(Debug, Clone, Default)]
pub struct HookLog(pub Vec<HookRun>);

impl HookLog {
    /// Builds an empty hook log.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Appends one finished hook execution.
    pub fn push(&mut self, run: HookRun) {
        self.0.push(run);
    }
}

/// One job progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobProgress {
    /// The finished count.
    pub done: usize,
    /// The total count.
    pub total: Option<usize>,
}

/// Returns the painted closing.
pub fn finished(
    kind: &TaskKind,
    done: usize,
    total: Option<usize>,
    note: &TaskNote,
) -> Option<String> {
    let raw = match (kind, note) {
        (TaskKind::Fetch { url }, TaskNote::Cached) => Some(format!("{} ({CACHED})", url.as_str())),
        (TaskKind::Fetch { url }, _) => match total {
            Some(known) => Some(format!(
                "downloaded {} ({} of {})",
                url_tail(url.as_str()),
                human_units(done),
                human_units(known)
            )),
            None => Some(url.as_str().to_owned()),
        },
        (TaskKind::PoolFill, _) => Some(pool(POOL_FILL, done, total)),
        (TaskKind::PoolPrune, _) => Some(pool(POOL_PRUNE, done, total)),
        (TaskKind::Archive { path }, TaskNote::Members(members)) => Some(format!(
            "collecting artifacts: {} ({members} members)",
            path.display()
        )),
        (TaskKind::Archive { path }, _) => {
            Some(format!("collecting artifacts: {}", path.display()))
        }
        (TaskKind::Pack { path }, TaskNote::Members(members)) => {
            Some(format!("packing {} ({members} members)", path.display()))
        }
        (TaskKind::Pack { path }, _) => Some(format!("packing {}", path.display())),
        (TaskKind::Write, _) => None,
        (TaskKind::Hook { .. }, _) => None,
    };
    raw.map(|text| text.dim())
}

/// Returns the painted hook execution for one finished hook.
pub fn executed(argv: &[String]) -> String {
    format!("> {}", argv.join(" ")).dim()
}

/// Returns the painted error block.
pub fn error_text(error: &Error) -> Vec<String> {
    let mut out = vec![format!("{} {}", "Error:".fail(), error.summary())];
    if !error.closing().is_empty() {
        out.push(error.closing().to_owned());
    }
    out
}

/// Returns the display label.
pub(crate) fn task_label(kind: &TaskKind) -> String {
    match kind {
        TaskKind::Fetch { url } => url.as_str().to_owned(),
        TaskKind::PoolFill => POOL_FILL.to_owned(),
        TaskKind::PoolPrune => POOL_PRUNE.to_owned(),
        TaskKind::Archive { path } => {
            format!("{COLLECTING}: {}", path.display())
        }
        TaskKind::Pack { path } => {
            format!("{PACKING} {}", path.display())
        }
        TaskKind::Write => "write".to_owned(),
        TaskKind::Hook { argv, .. } => Arg::join(argv),
    }
}

/// Returns the pool progress.
fn pool(label: &str, done: usize, total: Option<usize>) -> String {
    match total {
        Some(known) => format!("{label} ({done} of {known} files)"),
        None => label.to_owned(),
    }
}

/// Returns the last URL segment.
fn url_tail(url: &str) -> &str {
    url.rsplit('/').next().unwrap_or(url)
}

/// Formats one count with a binary unit.
fn human_units(value: usize) -> String {
    const STEP: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if value < 1024 {
        return format!("{value} B");
    }
    let mut scaled = value as f64;
    let mut unit = 0;
    while scaled >= STEP && unit + 1 < UNITS.len() {
        scaled /= STEP;
        unit += 1;
    }
    format!("{scaled:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use confit_events::protocol::{TaskKind, TaskNote};

    use crate::shared::state::finished;

    #[test]
    fn finish_stays_quiet_for_write_and_hook() {
        let write = finished(&TaskKind::Write, 1, Some(1), &TaskNote::Closed);
        assert_eq!(write, None);
        let hook = finished(
            &TaskKind::Hook {
                position: 1,
                total: 1,
                argv: Vec::new(),
            },
            0,
            None,
            &TaskNote::Closed,
        );
        assert_eq!(hook, None);
    }
}
