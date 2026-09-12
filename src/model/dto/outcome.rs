//! Outcome
//!
//! Evaluated plan data returned for presentation to render.

use super::super::state::plan::Plan;
use super::diff::{ArtifactDetail, DiskDetail, PlanSummary};
use super::warning::PlanWarning;

/// Evaluated plan plus warnings and counts for presentation to render.
#[derive(Debug)]
pub struct PlanOutcome {
    /// Merged plan ready for payload rendering.
    pub plan: Plan,
    /// Filesystem warnings in plan order.
    pub warnings: Vec<PlanWarning>,
    /// Lifecycle counts for the summary line.
    pub summary: PlanSummary,
    /// Per artifact entry diffs in plan order.
    pub details: Vec<ArtifactDetail>,
    /// Per artifact disk diffs in plan order.
    pub disk: Vec<DiskDetail>,
    /// Winner over loser expansion for changed lines.
    pub conflicts: bool,
}

/// Evaluated summary data for presentation to render.
#[derive(Debug)]
pub struct StatusOutcome {
    /// Merged plan backing the summary rendering.
    pub plan: Plan,
    /// Filesystem warnings in plan order.
    pub warnings: Vec<PlanWarning>,
    /// Lifecycle counts for the summary line.
    pub summary: PlanSummary,
    /// Per artifact entry diffs in plan order.
    pub details: Vec<ArtifactDetail>,
    /// Per artifact disk diffs in plan order.
    pub disk: Vec<DiskDetail>,
    /// Winner over loser expansion for changed lines.
    pub conflicts: bool,
}
