//! Actions
//!
//! End to end flows for each CLI action composed from services. Each flow
//! gathers data and returns it for presentation to render.

pub mod plan;

pub use plan::plan;

use std::path::Path;
use std::path::PathBuf;

use crate::cli::{PlanArgs, StatusArgs};
use crate::error::Result;
use crate::model::dto::diff::ArtifactDetail;
use crate::model::dto::diff::DiskDetail;
use crate::model::dto::diff::PlanSummary;
use crate::model::dto::outcome::PlanOutcome;
use crate::model::dto::outcome::StatusOutcome;
use crate::model::dto::warning::PlanWarning;
use crate::model::state::plan::Plan;
use crate::repository::Filesystem;
use crate::services::diff::{detail, detail_disk};
use crate::services::path::resolve_root;
use crate::services::plan::{
    diff, disk_warnings, evaluate_profile, load_state, serialize, snapshot_current, summarize,
    write_plan,
};
use crate::services::render::render_baseline;

/// Shared products for the plan plus status flows.
///
/// Holds the evaluated plan plus counts plus diffs plus disk state plus
/// warnings. The conflicts flag travels with the products so both runners
/// forward the same value into their outcome.
struct Prepared {
    plan: Plan,
    summary: PlanSummary,
    details: Vec<ArtifactDetail>,
    disk: Vec<DiskDetail>,
    warnings: Vec<PlanWarning>,
    conflicts: bool,
}

/// Resolves the root plus profile plus state into shared plan products.
///
/// # Arguments
///
/// * `profile` - profile path holding the Lua entry point.
/// * `root` - configured root override.
/// * `state` - state file override.
/// * `conflicts` - winner over loser expansion carried into the outcome.
/// * `fs` - filesystem backend holding state plus snapshots.
///
/// # Returns
///
/// Evaluated plan plus summary plus details plus disk comparison plus warnings.
///
/// # Errors
///
/// Fails with evaluation plus state plus diff plus render errors.
fn prepare<F: Filesystem>(
    profile: &Path,
    root: &Option<PathBuf>,
    state: Option<&Path>,
    conflicts: bool,
    fs: &F,
) -> Result<Prepared> {
    let resolved = resolve_root(root, profile)?;
    let graph = evaluate_profile(&resolved, profile)?;
    let previous = load_state(fs, state)?;
    let plan = plan(
        &graph,
        &resolved.display().to_string(),
        &profile.display().to_string(),
    )?;
    let disk_snapshots = snapshot_current(fs, &plan);
    let counts = diff(&plan, &previous);
    let summary = summarize(&counts);
    let details = detail(&plan, &previous);
    let rendered = render_baseline(&plan, &resolved, fs)?;
    let disk = detail_disk(&plan, &rendered, &disk_snapshots);
    let warnings = disk_warnings(&plan, &previous, &disk_snapshots, &rendered);
    Ok(Prepared {
        plan,
        summary,
        details,
        disk,
        warnings,
        conflicts,
    })
}

/// Evaluates the profile and builds the plan outcome for presentation.
///
/// # Arguments
///
/// * `args` - plan flags holding profile plus root plus output plus state plus conflicts.
/// * `fs` - filesystem backend holding state plus snapshots plus plan writes.
///
/// # Returns
///
/// Plan plus summary plus details plus disk comparison plus warnings.
///
/// # Errors
///
/// Fails with evaluation plus state plus diff plus render plus serialization errors.
///
/// # Examples
/// ```rust,no_run
/// use std::path::PathBuf;
/// use confit::actions::run_plan;
/// use confit::cli::PlanArgs;
/// use confit::repository::MemoryFilesystem;
///
/// let args = PlanArgs {
///     profile: PathBuf::from("examples/0-basic_tool/profile.lua"),
///     root: Some(PathBuf::from("examples/0-basic_tool")),
///     output: None,
///     state: None,
///     conflicts: false,
/// };
/// let fs = MemoryFilesystem::default();
/// let outcome = match run_plan(&args, &fs) {
///     Ok(outcome) => outcome,
///     Err(error) => panic!("plan runs: {error}"),
/// };
/// assert_eq!(outcome.plan.artifacts.len(), 2);
/// assert!(outcome.warnings.is_empty());
/// ```
pub fn run_plan<F: Filesystem>(args: &PlanArgs, fs: &F) -> Result<PlanOutcome> {
    let prepared = prepare(
        &args.profile,
        &args.root,
        args.state.as_deref(),
        args.conflicts,
        fs,
    )?;
    if let Some(dest) = &args.output {
        let bytes = serialize(&prepared.plan)?;
        write_plan(fs, bytes.as_bytes(), dest)?;
    }
    Ok(PlanOutcome {
        plan: prepared.plan,
        summary: prepared.summary,
        details: prepared.details,
        disk: prepared.disk,
        conflicts: prepared.conflicts,
        warnings: prepared.warnings,
    })
}

/// Evaluates the profile and builds the status outcome for presentation.
///
/// # Arguments
///
/// * `args` - status flags holding profile plus root plus state plus conflicts.
/// * `fs` - filesystem backend holding state plus snapshots.
///
/// # Returns
///
/// Plan plus summary plus details plus disk comparison plus warnings.
///
/// # Errors
///
/// Fails with evaluation plus state plus diff plus render errors.
pub fn run_status<F: Filesystem>(args: &StatusArgs, fs: &F) -> Result<StatusOutcome> {
    let prepared = prepare(
        &args.profile,
        &args.root,
        args.state.as_deref(),
        args.conflicts,
        fs,
    )?;
    Ok(StatusOutcome {
        plan: prepared.plan,
        summary: prepared.summary,
        details: prepared.details,
        disk: prepared.disk,
        conflicts: prepared.conflicts,
        warnings: prepared.warnings,
    })
}
