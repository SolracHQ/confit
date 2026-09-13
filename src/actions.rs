//! Actions
//!
//! End to end flows for each CLI action composed from services. Each flow
//! gathers data and returns it for presentation to render.

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
    build_plan, diff, disk_warnings, load_state, snapshot_current, summarize, write_plan,
};
use crate::services::render::render_baseline;

/// Shared products for the plan plus status flows.
///
/// Holds the evaluated plan plus counts plus diffs plus disk state plus
/// warnings.
struct Prepared {
    plan: Plan,
    summary: PlanSummary,
    details: Vec<ArtifactDetail>,
    disk: Vec<DiskDetail>,
    warnings: Vec<PlanWarning>,
}

/// Resolves the root plus profile plus state into shared plan products.
///
/// # Arguments
///
/// * `profile` - profile path holding the Lua entry point.
/// * `root` - configured root override.
/// * `state` - state file override.
/// * `plugins` - external plugins folder, empty keeps embedded defaults only.
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
    plugins: Option<&Path>,
    fs: &F,
) -> Result<Prepared> {
    let resolved = resolve_root(root, profile)?;
    let graph = crate::binding::evaluate_with_plugins(&resolved, profile, plugins)?;
    let previous = load_state(fs, state)?;
    let plan = build_plan(
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
    })
}

/// Evaluates the profile and builds the plan outcome for presentation.
///
/// # Arguments
///
/// * `args` - plan flags holding profile plus root plus output plus state.
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
    run_plan_with_plugins(args, fs, None)
}

/// Evaluates the profile and builds the plan outcome with plugins available.
///
/// # Arguments
///
/// * `args` - plan flags holding profile plus root plus output plus state.
/// * `fs` - filesystem backend holding state plus snapshots plus plan writes.
/// * `plugins` - external plugins folder, empty keeps embedded defaults only.
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
/// use confit::actions::run_plan_with_plugins;
/// use confit::cli::PlanArgs;
/// use confit::repository::MemoryFilesystem;
///
/// let args = PlanArgs {
///     profile: PathBuf::from("examples/0-basic_tool/profile.lua"),
///     root: Some(PathBuf::from("examples/0-basic_tool")),
///     output: None,
///     state: None,
/// };
/// let fs = MemoryFilesystem::default();
/// let outcome = match run_plan_with_plugins(&args, &fs, None) {
///     Ok(outcome) => outcome,
///     Err(error) => panic!("plan runs: {error}"),
/// };
/// assert_eq!(outcome.plan.artifacts.len(), 2);
/// assert!(outcome.warnings.is_empty());
/// ```
pub fn run_plan_with_plugins<F: Filesystem>(
    args: &PlanArgs,
    fs: &F,
    plugins: Option<&Path>,
) -> Result<PlanOutcome> {
    let prepared = prepare(
        &args.profile,
        &args.root,
        args.state.as_deref(),
        plugins,
        fs,
    )?;
    if let Some(dest) = &args.output {
        let bytes = serde_json::to_string_pretty(&prepared.plan)?;
        write_plan(fs, bytes.as_bytes(), dest)?;
    }
    Ok(PlanOutcome {
        plan: prepared.plan,
        summary: prepared.summary,
        details: prepared.details,
        disk: prepared.disk,
        warnings: prepared.warnings,
    })
}

/// Evaluates the profile and builds the status outcome for presentation.
///
/// # Arguments
///
/// * `args` - status flags holding profile plus root plus state.
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
    run_status_with_plugins(args, fs, None)
}

/// Evaluates the profile and builds the status outcome with plugins available.
///
/// # Arguments
///
/// * `args` - status flags holding profile plus root plus state.
/// * `fs` - filesystem backend holding state plus snapshots.
/// * `plugins` - external plugins folder, empty keeps embedded defaults only.
///
/// # Returns
///
/// Plan plus summary plus details plus disk comparison plus warnings.
///
/// # Errors
///
/// Fails with evaluation plus state plus diff plus render errors.
pub fn run_status_with_plugins<F: Filesystem>(
    args: &StatusArgs,
    fs: &F,
    plugins: Option<&Path>,
) -> Result<StatusOutcome> {
    let prepared = prepare(
        &args.profile,
        &args.root,
        args.state.as_deref(),
        plugins,
        fs,
    )?;
    Ok(StatusOutcome {
        plan: prepared.plan,
        summary: prepared.summary,
        details: prepared.details,
        disk: prepared.disk,
        warnings: prepared.warnings,
    })
}
