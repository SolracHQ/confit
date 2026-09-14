//! Diff
//!
//! Shared diff shapes for entry comparison and disk comparison.

/// One diff sigil: the mark paints in front of a change line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sigil {
    /// Shared line, present on both sides.
    Context,
    /// Desired-only line.
    Add,
    /// Disk-only or previous-only line.
    Remove,
    /// Changed pair, previous then desired.
    Update,
    /// File markers (`---`, `+++`) and hunk ranges (`@@`).
    Header,
}

/// One structured change line for disk diffs.
///
/// Keyed lines fill `key` with the dotted path and `old`/`new` with leaf values. Text lines
/// fill `key` with the line body and leave `old`/`new` empty. Header lines fill `key` with the
/// verbatim marker.
///
/// Keyed updates fill both `old` and `new`; additions fill `new` alone; removals fill `old`
/// alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeLine {
    /// Transition shape for painting and rendering.
    pub sigil: Sigil,
    /// Dotted key path, or line body, or verbatim header marker.
    pub key: String,
    /// Previous value for updates and removals.
    pub old: Option<String>,
    /// Desired value for updates and additions.
    pub new: Option<String>,
}

impl ChangeLine {
    /// Builds a keyed change line.
    ///
    /// # Arguments
    ///
    /// * `sigil` - the transition shape for painting and rendering.
    /// * `key` - the dotted key path.
    /// * `old` - the previous value, holding `None` for additions.
    /// * `new` - the desired value, holding `None` for removals.
    ///
    /// # Returns
    ///
    /// The keyed line carrying the given transition.
    pub fn keyed(sigil: Sigil, key: &str, old: Option<&str>, new: Option<&str>) -> Self {
        Self {
            sigil,
            key: key.to_string(),
            old: old.map(str::to_string),
            new: new.map(str::to_string),
        }
    }

    /// Builds a text change line.
    ///
    /// # Arguments
    ///
    /// * `sigil` - the transition shape for painting and rendering.
    /// * `body` - the line body carried as the key.
    ///
    /// # Returns
    ///
    /// The text line carrying the given body.
    pub fn text(sigil: Sigil, body: &str) -> Self {
        Self {
            sigil,
            key: body.to_string(),
            old: None,
            new: None,
        }
    }

    /// Builds a header marker line.
    ///
    /// # Arguments
    ///
    /// * `marker` - the verbatim header marker carried as the key.
    ///
    /// # Returns
    ///
    /// The header line carrying the given marker.
    pub fn header(marker: &str) -> Self {
        Self {
            sigil: Sigil::Header,
            key: marker.to_string(),
            old: None,
            new: None,
        }
    }

    /// Reports whether the line holds a header marker.
    ///
    /// # Returns
    ///
    /// `true` for header sigils.
    pub fn is_header(&self) -> bool {
        self.sigil == Sigil::Header
    }
}

/// Per-document lifecycle status against the previous state.
///
/// Every plan document maps to exactly one status; status derives from `data_hash` alone, with
/// entry contents feeding display only, so hash-equal documents stay collapsed while entry
/// rendering walks snapshots.
///
/// # Returns
///
/// The same three outcomes as the count diff: absent previous means create, differing
/// `data_hash` means update, equal hash means no change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// Document absent from the previous state.
    Create,
    /// Document present with a differing `data_hash`.
    Update,
    /// Document present with an equal `data_hash`.
    Unchanged,
}

/// Per-entry change shape.
///
/// `Changed` always names `from` (previous) then `to` (desired); file/link values hold
/// truncated (12-char) `data_hash`es as display text, so large contents stay out of the
/// summary.
///
/// # Returns
///
/// The transition for one labeled setting: added carries the desired value, changed carries
/// previous then desired, removed carries the previous value, matching carries the shared
/// value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeKind {
    /// Setting present in desired, absent in previous; holds desired value.
    Added {
        /// Desired value (or truncated hash for file/link).
        value: String,
    },
    /// Setting present in both with differing values.
    Changed {
        /// Previous value (or previous truncated hash).
        from: String,
        /// Desired value (or current truncated hash).
        to: String,
    },
    /// Setting present in previous, absent in desired; holds previous value.
    Removed {
        /// Previous value.
        value: String,
    },
    /// Setting present in both with equal values; holds shared value.
    Unchanged {
        /// Shared value.
        value: String,
    },
}

/// One labeled entry transition.
///
/// # Returns
///
/// The label plus change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryChange {
    /// Setting label: `alias cat`, `_ZO_DOCTOR`, `profile PATH`,
    /// `init[0]`, `tools.bat`, `vars.key`, `src`, `content`, `target`.
    pub label: String,
    /// Transition with values.
    pub change: ChangeKind,
}

/// Per-document entry diff.
///
/// `key` reads `"kind:path"` with the lowercase kind, matching services plan diff; `entries`
/// hold desired order first (aliases sorted, env/profile declaration order, init index order,
/// toml dotted-path sorted, template vars sorted plus `src`, file/link single line), then
/// removed entries sorted by label; absent previous or absent snapshot yields all-added
/// entries.
///
/// # Returns
///
/// The document key plus status plus ordered entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentDetail {
    /// Document key `"kind:path"`.
    pub key: String,
    /// Lifecycle status from `data_hash` comparison.
    pub status: DocumentStatus,
    /// Ordered entry transitions.
    pub entries: Vec<EntryChange>,
}

/// On-disk diff for one document: disk bytes against the baseline rendering.
///
/// Holds the document key plus structured change lines. Presentation renders the section body:
/// keyed lines for structured kinds, unified hunks for byte kinds. Stays empty while disk
/// matches the baseline.
///
/// `key` reads `"kind:path"` matching [`detail`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskDetail {
    /// Document key `"kind:path"`.
    pub key: String,
    /// Structured diff lines for the disk section.
    pub lines: Vec<ChangeLine>,
}

/// Per-document diff counts of a plan against the previous state.
///
/// Create plus update sum to changed documents. Delete counts previous-state keys matching
/// zero plan documents, so it tracks removals apart from the plan count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSummary {
    /// Documents absent from the previous state.
    pub create: usize,
    /// Documents whose `data_hash` differs from the previous state.
    pub update: usize,
    /// Previous-state keys matching zero plan documents.
    pub delete: usize,
}

/// Plan counts shaped for presentation.
///
/// Projects [`DiffSummary`] into the shape presentation renders. Holds counts alone. Details
/// travel beside it as entry diffs.
///
/// # Examples
/// ```rust
/// use confit::model::state::document::Document;
/// use confit::model::state::document::DocumentData;
/// use confit::model::state::document::DocumentKind;
/// use confit::model::state::plan::Plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::model::state::State;
/// use confit::services::plan::{diff, summarize};
///
/// fn file_document(path: &str) -> Document {
///     Document {
///         kind: DocumentKind::Text,
///         path: path.into(),
///         data: DocumentData::Text { content: "hi".into() },
///         data_hash: "hash".into(),
///     }
/// }
/// let plan = Plan { version: PLAN_VERSION, created_at: String::new(), root: String::new(), profile: String::new(), documents: vec![file_document("a"), file_document("b")] };
/// let counts = diff(&plan, &State::empty());
/// let summary = summarize(&counts);
/// assert_eq!(summary.create, 2);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSummary {
    /// Documents absent from the previous state.
    pub create: usize,
    /// Documents whose data differs from the previous state.
    pub update: usize,
    /// Previous-state keys matching zero plan documents.
    pub delete: usize,
}
