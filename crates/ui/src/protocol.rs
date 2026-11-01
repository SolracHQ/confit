//! Message protocol between workers and the render thread.

/// Fact for one run step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Marks one operation opening.
    Started {
        /// Stable operation id.
        id: String,
        /// Human label for the spinner row.
        label: String,
    },
    /// Marks one operation advance.
    Progress {
        /// Stable operation id.
        id: String,
        /// Human label for the spinner row.
        label: String,
        /// Finished units including this update.
        done: u64,
        /// Total units when known.
        total: Option<u64>,
    },
    /// Marks one operation end.
    Finished {
        /// Stable operation id.
        id: String,
        /// Human label for the spinner row.
        label: String,
    },
}

/// Message for the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToUi {
    /// Progress fact for the spinner.
    Progress(Event),
    /// Line for stderr above the spinner.
    Print(String),
    /// Question for stdin with the answer on the return channel.
    Prompt(String),
}

/// Message from the render thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromUi {
    /// Line read from stdin without the trailing newline.
    Answer(String),
}
