//! Component display strings.

use confit_differ::change::IncomparableReason;
use extend::ext;

/// Cached fetch suffix.
pub(crate) const CACHED: &str = "cached";
/// Approval answer.
pub(crate) const YES: &str = "yes";
/// Decline answer.
pub(crate) const NO: &str = "no";
/// Row expansion answer.
pub(crate) const DETAILS: &str = "details";
/// Exact rescue title.
pub(crate) const RESTORE_EXACT_TITLE: &str = "To restore previous exact disk state:";
/// Undo title.
pub(crate) const RESTORE_TITLE: &str = "To restore previous state:";
/// Drifted undo title.
pub(crate) const RESTORE_DRIFTED_TITLE: &str = "To restore previous state (dropping disk changes):";
/// Exact rescue command.
pub(crate) const RESTORE_EXACT_ARG: &str = "restore '@!'";
/// Undo command.
pub(crate) const RESTORE_UNDO_ARG: &str = "restore %1";
/// Cancellation line.
pub(crate) const CANCELLED: &str = "Cancelled.";
/// Results table title.
pub(crate) const RESULTS: &str = "Results";
/// Reason word for an unreadable path.
pub(crate) const REASON_UNREADABLE: &str = "unreadable";
/// Reason word for a directory at the managed path.
pub(crate) const REASON_DIR_THERE: &str = "directory blocks the managed path";
/// Reason word for a blocked ancestor.
pub(crate) const REASON_BLOCKED_BY_DIR: &str = "ancestor blocks the path";
/// Reason word for a special file at the path.
pub(crate) const REASON_BLOCKED_BY_SPECIAL_FILE: &str = "special file blocks the path";

#[ext]
pub impl IncomparableReason {
    /// Returns the reason words.
    fn text(&self) -> &'static str {
        match self {
            Self::Unreadable => REASON_UNREADABLE,
            Self::DirThere => REASON_DIR_THERE,
            Self::BlockedByDir => REASON_BLOCKED_BY_DIR,
            Self::BlockedBySpecialFile => REASON_BLOCKED_BY_SPECIAL_FILE,
        }
    }
}
