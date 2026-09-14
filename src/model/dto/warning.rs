//! Warning
//!
//! Filesystem warning shapes carried beside plan counts.

/// Filesystem warning shape for one document path.
/// Stays data; presentation renders each warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningKind {
    /// Untracked path holding bytes; creation overwrites it.
    OverwriteUntracked,
    /// The record matches desired but disk bytes differ: manual edits will
    /// be overwritten.
    ManualModification,
    /// Path with failing reads; holds the snapshot reason.
    Unreadable {
        /// Snapshot reason naming the path and the IO failure.
        reason: String,
    },
    /// Same path declared with different bytes; holds owner names in order.
    DeclarationConflict {
        /// Owner names sharing the path, sorted for stable output.
        owners: Vec<String>,
    },
}

/// One filesystem warning attached to a document path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanWarning {
    /// Document path the warning belongs to.
    pub path: String,
    /// Warning shape.
    pub kind: WarningKind,
}
