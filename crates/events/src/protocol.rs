//! Worker progress, prompt, and diff events for the render thread.

use std::path::{Path, PathBuf};

use confit_core::error::Error;
use confit_differ::diff::Diff;
use confit_model::{arg::Arg, url::Url};

/// One apply outcome for a route.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApplyOutcome {
    /// A route that landed on disk.
    Added,
    /// A route that changed on disk.
    Changed,
    /// A route that left the disk.
    Removed,
    /// A route that failed to apply.
    Failed,
}

impl ApplyOutcome {
    /// Counts outcomes by variant.
    pub fn count<I>(iter: I) -> (usize, usize, usize, usize)
    where
        I: IntoIterator<Item = ApplyOutcome>,
    {
        let mut added = 0;
        let mut changed = 0;
        let mut removed = 0;
        let mut failed = 0;
        for outcome in iter {
            match outcome {
                ApplyOutcome::Added => added += 1,
                ApplyOutcome::Changed => changed += 1,
                ApplyOutcome::Removed => removed += 1,
                ApplyOutcome::Failed => failed += 1,
            }
        }
        (added, changed, removed, failed)
    }
}

/// The counts for one effective apply.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Applied {
    /// The addition rows the execution wrote.
    pub added: usize,
    /// The change rows the execution wrote.
    pub changed: usize,
    /// The removal rows the execution removed.
    pub removed: usize,
    /// The written rows in view order.
    pub changes: Vec<AppliedChange>,
}

/// One recorded write.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AppliedChange {
    /// The expanded host path the execution wrote.
    pub path: String,
    /// The outcome the execution recorded for the path.
    pub outcome: ApplyOutcome,
}

impl Applied {
    /// Records one written row.
    pub fn push(&mut self, path: &Path, outcome: ApplyOutcome) {
        self.changes.push(AppliedChange {
            path: path.display().to_string(),
            outcome,
        });
    }
}

/// One background job behind the progress bars.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TaskKind {
    /// One download.
    Fetch { url: Url },
    /// One fill of missing pool entries.
    PoolFill,
    /// One prune of unreferenced pool entries.
    PoolPrune,
    /// One archive extract.
    Archive { path: PathBuf },
    /// One bundle pack.
    Pack { path: PathBuf },
    /// The write execution over the effective rows.
    Write,
    /// One hook execution.
    Hook {
        /// The hook position in the execution.
        position: usize,
        /// The hook count for the execution.
        total: usize,
        /// The hook arguments.
        argv: Vec<Arg>,
    },
}

/// One write row in a progress update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowUpdate {
    /// Expanded host path bytes.
    pub path: String,
    /// The recorded outcome.
    pub outcome: ApplyOutcome,
}

/// One closing fact for a finished job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskNote {
    /// The fetch served the body from the cache.
    Cached,
    /// The archive landed with the member count.
    Members(usize),
    /// The spawned command for one hook execution.
    Spawned(Vec<String>),
    /// The job landed with no extra fact.
    Closed,
}

/// One progress update for a background job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// The job opened with the known total when present.
    Started { total: Option<usize> },
    /// The job advanced with the finished count and the total.
    Progress {
        /// The finished count for this update.
        done: usize,
        /// The total count when known.
        total: Option<usize>,
        /// The applied row in a write update.
        row: Option<RowUpdate>,
    },
    /// The job closed with its closing fact.
    Finished { note: TaskNote },
}

/// One user-visible work phase for a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The profile evaluation execution.
    Evaluation,
    /// The bundle packaging execution.
    Packaging,
    /// The blob and state persistence execution.
    Persisting,
    /// The write execution over the effective rows.
    ApplyingChanges,
}

/// One message to the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToUi {
    /// One phase start with its title.
    PhaseStarted(Phase),
    /// One progress update for a background job.
    Task {
        /// The job behind the update.
        kind: TaskKind,
        /// The update for the job.
        state: TaskState,
    },
    /// One check preview request.
    DesiredPreview(Diff),
    /// One commit preview request with the approval reply.
    ShowAndConfirm {
        /// The diff behind the preview.
        diff: Diff,
        /// The skip-confirmation flag.
        force: bool,
    },
    /// The write results with per-row outcomes.
    WriteResults(Applied),
    /// One question request.
    Prompt(String),
    /// One failure notice with no reply.
    Error(Box<Error>),
    /// One close request.
    Done {
        /// The previous entry flag.
        undo: bool,
        /// The rescue save flag.
        rescued: bool,
        /// The drift view flag.
        drifted: bool,
    },
}

/// One message back from the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromUi {
    /// One stdin line without the trailing newline.
    Answer(String),
    /// The user approved the confirmation. Anything else is a no.
    Approved,
    /// The cancelled prompt reply.
    Close,
}
