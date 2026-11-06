//! Event and plan types for the worker and render threads.

use std::path::PathBuf;

use confit_model::routes::Route;

/// One action for a route in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanAction {
    /// A route in the desired state and absent from
    /// the recorded state.
    Add,
    /// A route in both states with new content.
    Change,
    /// A route absent from the desired state and present
    /// in the recorded state.
    Remove,
    /// A route with identical content in both states.
    Same,
}

/// One comparison that produces a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeSource {
    /// A change found in the desired state.
    Desired,
    /// A change found in the recorded state.
    Recorded,
    /// A change found on disk.
    Disk,
}

/// One difference for a route in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    /// A leaf value with old and new text.
    LeafValue {
        /// The leaf key in the document.
        key: String,
        /// The recorded text.
        old: String,
        /// The desired text.
        new: String,
        /// The comparison that produced the change.
        source: ChangeSource,
    },
    /// A text line added or removed.
    TextLine {
        /// The added or removed line.
        line: String,
        /// The comparison that produced the change.
        source: ChangeSource,
    },
    /// A link with old and new targets.
    LinkTarget {
        /// The recorded target.
        old: String,
        /// The desired target.
        new: String,
        /// The comparison that produced the change.
        source: ChangeSource,
    },
    /// An opaque blob with old and new hashes.
    OpaqueHash {
        /// The recorded hash.
        old: String,
        /// The desired hash.
        new: String,
        /// The comparison that produced the change.
        source: ChangeSource,
    },
    /// A file mode with old and new bits.
    Mode {
        /// The recorded mode bits.
        old: u32,
        /// The desired mode bits.
        new: u32,
        /// The comparison that produced the change.
        source: ChangeSource,
    },
    /// A tree member with an action.
    TreeMember {
        /// The member path relative to the tree root.
        relative: String,
        /// The action for the member.
        action: PlanAction,
        /// The comparison that produced the change.
        source: ChangeSource,
    },
    /// A document with a new kind.
    KindChange {
        /// The recorded kind.
        old: String,
        /// The desired kind.
        new: String,
    },
}

/// One route in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    /// The route path as text.
    pub route: String,
    /// The document kind as text.
    pub kind: String,
    /// The action for the route.
    pub action: PlanAction,
    /// The differences for the route.
    pub changes: Vec<Change>,
    /// Whether disk differs from the recorded state.
    pub drift: bool,
    /// Whether the path needs rescue capture.
    pub rescue: bool,
}

/// The full preview of one apply run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// One entry per route.
    pub entries: Vec<PlanEntry>,
    /// Human-readable warnings for the run.
    pub warnings: Vec<String>,
    /// Whether no applied state exists yet.
    pub first_run: bool,
}

/// One apply outcome for a route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// A route that landed on disk.
    Added,
    /// A route that left the disk.
    Removed,
}

/// One message to the render thread.
///
/// - Each run step names one operation; the backend owns the words,
///   the widgets, and the units.
/// - Plans, prints, prompts, and confirmations share the channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToUi {
    /// A fetch that starts.
    FetchStarted {
        /// The downloaded URL.
        url: String,
    },
    /// A fetch in flight.
    FetchProgress {
        /// The downloaded URL.
        url: String,
        /// The finished bytes through this update.
        done: usize,
        /// The byte count for the whole body, absent
        /// when the source omits it.
        total: Option<usize>,
    },
    /// A fetch that lands.
    FetchFinished {
        /// The downloaded URL.
        url: String,
        /// Whether the bytes came from the cache.
        cached: bool,
    },
    /// A pool fill that starts.
    PoolFillStarted {
        /// The file count for the whole fill.
        total: usize,
    },
    /// A pool fill in flight.
    PoolFillProgress {
        /// The stored files through this update.
        done: usize,
        /// The file count for the whole fill.
        total: usize,
    },
    /// A pool fill that lands.
    PoolFillFinished,
    /// A pool prune that starts.
    PoolPruneStarted {
        /// The file count for the whole prune.
        total: usize,
    },
    /// A pool prune in flight.
    PoolPruneProgress {
        /// The removed files through this update.
        done: usize,
        /// The file count for the whole prune.
        total: usize,
    },
    /// A pool prune that lands.
    PoolPruneFinished,
    /// An archive extract that starts.
    ArchiveStarted {
        /// The archive path.
        path: PathBuf,
    },
    /// An archive extract that lands.
    ArchiveFinished {
        /// The archive path.
        path: PathBuf,
        /// The extracted member count.
        members: usize,
    },
    /// An apply that starts.
    ApplyStarted {
        /// The file count for the whole apply.
        total: usize,
    },
    /// An apply in flight.
    ApplyProgress {
        /// The applied route.
        route: Route,
        /// Whether the route landed or left.
        outcome: ApplyOutcome,
    },
    /// An apply that lands.
    ApplyFinished,
    /// A hook run that starts.
    HookStarted {
        /// The hook position in the run.
        position: usize,
        /// The hook count for the run.
        total: usize,
        /// The hook command line.
        argv: String,
    },
    /// A hook run that lands.
    HookFinished {
        /// The hook position in the run.
        position: usize,
        /// The hook count for the run.
        total: usize,
        /// The hook command line.
        argv: String,
    },
    /// One run plan as data for review.
    Plan(Plan),
    /// Text written to stderr above the spinner.
    Print(String),
    /// A question written to stderr, with the answer read from stdin.
    Prompt(String),
    /// A confirmation request.
    Confirm,
}

/// One message back from the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromUi {
    /// A line read from stdin with the trailing newline trimmed.
    Answer(String),
    /// The user approved the confirmation. Anything else is a no.
    Approved,
    /// The user dismissed the prompt.
    Close,
}
