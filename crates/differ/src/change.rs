//! Route changes with comparability states.

use std::path::PathBuf;

use confit_core::error::{ContextualResult, DocumentError, Require, WithContext, contextual};
use confit_core::sha::Sha;
use confit_model::document::FileMode;
use confit_model::routes::Route;
use serde::Serialize;

use crate::{content::ContentDiff, entries::Entry};

/// One present side of a route change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Side {
    /// Content state.
    pub sha: Sha,
    /// Permission bits.
    pub mode: FileMode,
}

/// One route change with before and after states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Change {
    /// Full route.
    pub path: Route,
    /// Host path.
    pub expanded: PathBuf,
    /// Route change kind.
    pub kind: ChangeKind,
    /// Disk comparability state.
    pub comparability: Comparability,
    /// Tree route.
    pub group: Option<Route>,
    /// Present state on the before side.
    pub before: Option<Side>,
    /// Present state on the after side.
    pub after: Option<Side>,
    /// The drift flag.
    pub drifted: bool,
}

/// One document outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    /// A new document.
    Addition,
    /// A modified document.
    Change,
    /// A removed document.
    Removal,
}

/// One disk comparability state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Comparability {
    /// Disk path supports comparison.
    Comparable,
    /// Disk path blocks comparison.
    Incomparable(IncomparableReason),
}

/// One incomparability reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IncomparableReason {
    /// Unreadable disk path.
    Unreadable,
    /// A directory is at the file route.
    DirThere,
    /// An ancestor blocks descent into the path.
    BlockedByDir,
    /// A socket, fifo, or device at the route.
    BlockedBySpecialFile,
}

impl Side {
    /// Builds one present side.
    pub fn new(sha: Sha, mode: FileMode) -> Self {
        Self { sha, mode }
    }
}

impl From<&Entry> for Side {
    fn from(entry: &Entry) -> Self {
        Self {
            sha: entry.sha,
            mode: entry.mode,
        }
    }
}

impl Change {
    /// Returns the opaque difference.
    pub(crate) fn opaque(&self) -> ContentDiff {
        ContentDiff::Opaque {
            before: self.before.map(|side| side.sha),
            after: self.after.map(|side| side.sha),
        }
    }

    /// Builds one change from before and after sides.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`]
    ///   when a base folder is missing.
    pub(crate) fn build(
        path: &Route,
        group: Option<Route>,
        before: Option<Side>,
        after: Option<Side>,
        unmanaged: bool,
    ) -> ContextualResult<Option<Self>> {
        let expanded = path
            .resolve()
            .ok_or_else(|| {
                contextual(DocumentError::ParseFailed {
                    require: Require::BaseFolder,
                })
            })
            .with_document(path.display())?;
        match (before, after) {
            (None, None) => Ok(None),
            (None, Some(found)) => Ok(Some(Self {
                path: path.clone(),
                expanded,
                kind: ChangeKind::Addition,
                comparability: Comparability::Comparable,
                group,
                before: None,
                after: Some(found),
                drifted: false,
            })),
            (Some(held), None) => Ok(Some(Self {
                path: path.clone(),
                expanded,
                kind: ChangeKind::Removal,
                comparability: Comparability::Comparable,
                group,
                before: Some(held),
                after: None,
                drifted: false,
            })),
            (Some(held), Some(wanted)) => {
                if held == wanted || unmanaged {
                    return Ok(None);
                }
                Ok(Some(Self {
                    path: path.clone(),
                    expanded,
                    kind: ChangeKind::Change,
                    comparability: Comparability::Comparable,
                    group,
                    before: Some(held),
                    after: Some(wanted),
                    drifted: false,
                }))
            }
        }
    }

    /// Builds one incomparable change.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::DocumentError::ParseFailed`]
    ///   when a base folder is missing.
    pub(crate) fn incomparable(
        path: &Route,
        group: Option<Route>,
        before: Option<Side>,
        after: Option<Side>,
        kind: ChangeKind,
        reason: IncomparableReason,
    ) -> ContextualResult<Self> {
        let expanded = path
            .resolve()
            .ok_or_else(|| {
                contextual(DocumentError::ParseFailed {
                    require: Require::BaseFolder,
                })
            })
            .with_document(path.display())?;
        Ok(Self {
            path: path.clone(),
            expanded,
            kind,
            comparability: Comparability::Incomparable(reason),
            group,
            before,
            after,
            drifted: false,
        })
    }
}

impl AsRef<ChangeKind> for Change {
    fn as_ref(&self) -> &ChangeKind {
        &self.kind
    }
}
