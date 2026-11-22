//! Command runners from flags to built plans and disk writes.

pub mod apply;
pub mod bundle;
pub mod check;
pub mod commit;
pub mod delete;
pub mod export;
pub mod init;
pub mod restore;

use confit_differ::diff::Diff;
use confit_model::manifest::Manifest;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
/// One artifact execution record.
pub struct ArtifactReport {
    /// The output text.
    pub output: String,
    /// The artifact manifest.
    pub manifest: Manifest,
}

/// One printable result per command.
pub enum ActionResult {
    /// The artifact execution.
    Artifact(ArtifactReport),
    /// The evaluated diff.
    Check(Diff),
    /// The write counts.
    Wrote(commit::WriteReport),
    /// The stored manifest.
    Export(ArtifactReport),
    /// The deleted state.
    Delete(delete::DeleteReport),
    /// One init item per outcome.
    Init(Vec<init::InitItem>),
}
