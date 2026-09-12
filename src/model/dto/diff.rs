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

/// Per-artifact lifecycle status against the previous state.
///
/// Every plan artifact maps to exactly one status; status derives from `data_hash` alone, with
/// entry contents feeding display only, so hash-equal artifacts report unchanged while entry
/// rendering walks snapshots.
///
/// # Returns
///
/// The same three outcomes as the count diff: absent previous means create, differing
/// `data_hash` means update, equal hash means unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactStatus {
    /// Artifact absent from the previous state.
    Create,
    /// Artifact present with a differing `data_hash`.
    Update,
    /// Artifact present with an equal `data_hash`.
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
/// previous then desired, removed carries the previous value, unchanged carries the shared
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

/// One labeled entry transition with winner attribution.
///
/// `tool` names the desired winner (removed entries carry `None`); `over` names the in-plan
/// shadow loser when shadow history names a different tool than the winner, else `None`, so the
/// conflicts flag renders losers exactly where history identifies one.
///
/// # Returns
///
/// The label plus change plus winner/loser tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryChange {
    /// Setting label: `alias cat`, `_ZO_DOCTOR`, `profile PATH`,
    /// `init[0]`, `tools.bat`, `vars.key`, `src`, `content`, `target`.
    pub label: String,
    /// Transition with values.
    pub change: ChangeKind,
    /// Winner tool (`None` for removed entries).
    pub tool: Option<String>,
    /// Shadow loser tool for `--conflicts` rendering.
    pub over: Option<String>,
}

/// Per-artifact entry diff.
///
/// `key` reads `"kind:path"` with the lowercase kind, matching services plan diff; `entries`
/// hold desired order first (aliases sorted, env/profile declaration order, init index order,
/// toml dotted-path sorted, template vars sorted plus `src`, file/link single line), then
/// removed entries sorted by label; absent previous or absent snapshot yields all-added
/// entries.
///
/// # Returns
///
/// The artifact key plus status plus ordered entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactDetail {
    /// Artifact key `"kind:path"`.
    pub key: String,
    /// Lifecycle status from `data_hash` comparison.
    pub status: ArtifactStatus,
    /// Ordered entry transitions.
    pub entries: Vec<EntryChange>,
}

/// On-disk diff for one artifact: disk bytes against the baseline rendering.
///
/// Holds the artifact key plus structured change lines. Presentation renders the section body:
/// keyed lines for structured kinds, unified hunks for byte kinds. Stays empty while disk
/// matches the baseline.
///
/// `key` reads `"kind:path"` matching [`detail`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskDetail {
    /// Artifact key `"kind:path"`.
    pub key: String,
    /// Structured diff lines for the disk section.
    pub lines: Vec<ChangeLine>,
}

/// Per-artifact diff counts of a plan against the previous state.
///
/// Create, update, and unchanged sum to the plan's artifact count; each artifact contributes
/// exactly one status. Delete counts previous-state keys matching zero plan artifacts, so it
/// tracks removals apart from the plan count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSummary {
    /// Artifacts absent from the previous state.
    pub create: usize,
    /// Artifacts whose `data_hash` differs from the previous state.
    pub update: usize,
    /// Artifacts whose `data_hash` matches the previous state.
    pub unchanged: usize,
    /// Previous-state keys matching zero plan artifacts.
    pub delete: usize,
}

/// Plan counts shaped for presentation.
///
/// Projects [`DiffSummary`] into the shape presentation renders. Holds counts alone. Details
/// travel beside it as entry diffs.
///
/// # Examples
/// ```rust
/// use confit::model::state::artifact::Artifact;
/// use confit::model::state::artifact::ArtifactData;
/// use confit::model::state::artifact::ArtifactKind;
/// use confit::model::state::plan::Plan;
/// use confit::model::state::plan::PLAN_VERSION;
/// use confit::model::state::State;
/// use confit::services::plan::{diff, summarize};
///
/// fn file_artifact(path: &str) -> Artifact {
///     Artifact {
///         kind: ArtifactKind::File,
///         path: path.into(),
///         data: ArtifactData::File { content: "hi".into() },
///         contributions: Vec::new(),
///         shadowed: Default::default(),
///         blame: Default::default(),
///         data_hash: "hash".into(),
///     }
/// }
/// let plan = Plan { version: PLAN_VERSION, created_at: String::new(), root: String::new(), profile: String::new(), artifacts: vec![file_artifact("a"), file_artifact("b")] };
/// let counts = diff(&plan, &State::empty());
/// let summary = summarize(&counts);
/// assert_eq!((summary.create, summary.update, summary.delete), (2, 0, 0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanSummary {
    /// Artifacts absent from the previous state.
    pub create: usize,
    /// Artifacts whose data differs from the previous state.
    pub update: usize,
    /// Previous-state keys matching zero plan artifacts.
    pub delete: usize,
}
