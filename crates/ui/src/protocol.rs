//! Messages between worker threads and the render thread.

/// The state of one run step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
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
        /// The unit count for the whole operation, absent when the source omits it.
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

/// One message to the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToUi {
    /// One run step event for the spinner.
    Progress(Event),
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
}
