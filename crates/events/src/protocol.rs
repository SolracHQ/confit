//! Worker progress, prompt, and diff events for the render thread.

use std::path::{Path, PathBuf};

use confit_core::error::Error;
use confit_differ::{diff::Diff, view::ViewKind};
use confit_model::{arg::Arg, url::Url};

/// An apply outcome for a route.
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

/// The counts for an apply.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Applied {
    /// The added change count.
    pub added: usize,
    /// The updated change count.
    pub changed: usize,
    /// The removed change count.
    pub removed: usize,
    /// The written changes in view order.
    pub changes: Vec<AppliedChange>,
}

/// A recorded write.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AppliedChange {
    /// The expanded host path.
    pub path: String,
    /// The write outcome.
    pub outcome: ApplyOutcome,
}

/// One created file set for the init tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitCreated {
    /// The scaffold root.
    pub root: PathBuf,
    /// The created files.
    pub files: Vec<PathBuf>,
}

impl Applied {
    /// Records a written row.
    pub fn push(&mut self, path: &Path, outcome: ApplyOutcome) {
        self.changes.push(AppliedChange {
            path: path.display().to_string(),
            outcome,
        });
    }
}

/// A background job behind the progress bars.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TaskKind {
    /// A download.
    Fetch {
        /// The fetch URL.
        url: Url,
    },
    /// A fill of missing pool entries.
    PoolFill,
    /// A prune of unreferenced pool entries.
    PoolPrune,
    /// An archive extract.
    Archive {
        /// The archive path.
        path: PathBuf,
    },
    /// A bundle pack.
    Pack {
        /// The bundle path.
        path: PathBuf,
    },
    /// The write execution.
    Write,
    /// A hook execution.
    Hook {
        /// The hook position in the execution.
        position: usize,
        /// The total hook count.
        total: usize,
        /// The hook arguments.
        argv: Vec<Arg>,
    },
}

/// A write row in a progress update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowUpdate {
    /// Expanded host path bytes.
    pub path: String,
    /// The recorded outcome.
    pub outcome: ApplyOutcome,
}

/// A closing fact for a finished job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskNote {
    /// The fetch served the body from the cache.
    Cached,
    /// The archive member count.
    Members(usize),
    /// The spawned command for a hook execution.
    Spawned(Vec<String>),
    /// The job landed with no extra fact.
    Closed,
}

/// A progress update for a background job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    /// The job opened.
    Started {
        /// The total count when known.
        total: Option<usize>,
    },
    /// The job advanced.
    Progress {
        /// The finished count for this update.
        done: usize,
        /// The total count when known.
        total: Option<usize>,
        /// The applied row in a write update.
        row: Option<RowUpdate>,
    },
    /// The job closed with its closing fact.
    Finished {
        /// The closing fact.
        note: TaskNote,
    },
}

/// A user-visible work phase for a title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The profile evaluation execution.
    Evaluation,
    /// The bundle packaging execution.
    Packaging,
    /// The blob and state persistence execution.
    Persisting,
    /// The write execution.
    ApplyingChanges,
}

/// A confirm prompt key for a blocking ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// The apply confirm prompt.
    Apply,
    /// The restore confirm prompt.
    Restore,
    /// The bundle confirm prompt.
    Bundle,
}

/// A message to the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToUi {
    /// A phase start with its title.
    PhaseStarted(Phase),
    /// A progress update for a background job.
    Task {
        /// The job behind the update.
        kind: TaskKind,
        /// The task state.
        state: TaskState,
    },
    /// A preview render.
    Preview {
        /// The full diff behind the preview.
        diff: Diff,
        /// The active view the backend renders.
        view: ViewKind,
    },
    /// A blocking details ask with no approval.
    AskForDetails,
    /// A blocking confirm ask.
    AskForConfirm {
        /// The prompt key.
        prompt: PromptKind,
        /// The skip-confirmation flag.
        force: bool,
    },
    /// The write results with per-row outcomes.
    WriteResults(Applied),
    /// The created files for the init tree.
    InitResults(InitCreated),
    /// A question request.
    Prompt(String),
    /// A failure notice with no reply.
    Error(Box<Error>),
    /// A close request.
    Done {
        /// The previous entry flag.
        undo: bool,
        /// The rescue save flag.
        rescued: bool,
        /// The drift view flag.
        drifted: bool,
    },
}

/// A message back from the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromUi {
    /// A stdin line without the trailing newline.
    Answer(String),
    /// An approval from the user. Anything else is a no.
    Approved,
    /// The cancelled prompt reply.
    Close,
}
