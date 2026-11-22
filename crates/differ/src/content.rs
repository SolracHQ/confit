//! Rendered differences for viewed routes.

use confit_core::sha::Sha;

/// Rendered difference for one viewed change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentDiff {
    /// Key changes.
    Structured {
        /// Changed keys.
        keys: Vec<KeyDiff>,
    },
    /// Line changes.
    Text {
        /// Changed lines.
        lines: Vec<LineDiff>,
    },
    /// Fallback difference.
    Opaque {
        /// State on the before side.
        before: Option<Sha>,
        /// State on the after side.
        after: Option<Sha>,
    },
}

/// One structured key change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyDiff {
    /// Dotted key name.
    pub key: String,
    /// Value on the before side.
    pub before: Option<serde_json::Value>,
    /// Value on the after side.
    pub after: Option<serde_json::Value>,
}

/// One text line change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDiff {
    /// The change tag.
    pub tag: LineTag,
    /// Line text.
    pub text: String,
}

/// One text line tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTag {
    /// Only the after side holds the line.
    Added,
    /// Only the before side holds the line.
    Removed,
}
