//! Event and plan types for the worker and render threads.

/// The state of one run step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgressEvent {
    /// An operation that starts.
    Started {
        /// The id shared by every update for this operation.
        id: String,
        /// The text shown on the spinner row.
        label: String,
    },
    /// An operation in progress.
    Progress {
        /// The id shared by every update for this operation.
        id: String,
        /// The text shown on the spinner row.
        label: String,
        /// The finished units through this update.
        done: u64,
        /// The unit count for the whole operation, absent
        /// when the source omits it.
        total: Option<u64>,
    },
    /// An operation that finishes.
    Finished {
        /// The id shared by every update for this operation.
        id: String,
        /// The text shown on the spinner row.
        label: String,
    },
}

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

/// One message to the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToUi {
    /// One run step event for the spinner.
    Progress(ProgressEvent),
    /// One run plan as data for review.
    Plan(Plan),
    /// Text written to stderr above the spinner.
    Print(String),
    /// A question written to stderr, with the answer read from stdin.
    Prompt(String),
}

/// One message back from the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromUi {
    /// A line read from stdin with the trailing newline trimmed.
    Answer(String),
    /// Whether the user confirmed the prompt.
    Confirm(bool),
}
