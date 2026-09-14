//! Outcome
//!
//! Evaluated plan data returned for presentation to render.

use super::super::state::plan::Plan;
use super::diff::{DiskDetail, DocumentDetail, PlanSummary};
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
    /// Per document entry diffs in plan order.
    pub details: Vec<DocumentDetail>,
    /// Per document disk diffs in plan order.
    pub disk: Vec<DiskDetail>,
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
    /// Per document entry diffs in plan order.
    pub details: Vec<DocumentDetail>,
    /// Per document disk diffs in plan order.
    pub disk: Vec<DiskDetail>,
}
