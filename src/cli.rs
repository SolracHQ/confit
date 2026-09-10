//! Command line: `plan` emits the plan (file or stdout) plus a summary,
//! `status` computes the summary in memory and prints it.
//!
//! Summaries and errors go to stderr; stdout carries only the plan payload.
//! Layering: this is the thin clap adapter over the `plan` service. IO stays
//! behind the `store` traits so tests can substitute memory fakes.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

use crate::error::{Error, Result};
use crate::lua::evaluate;
use crate::model::Plan;
use crate::store::{PlanFormat, StateStore};

/// Confit: declarative user-space state with plan-before-apply.
///
/// Parses the subcommand; `main` wires the filesystem backends and prints
/// the returned summary. Clap owns `--help`/`--version` rendering and its
/// usage exit code; service errors surface via the crate error type.
#[derive(Debug, Parser)]
#[command(name = "confit", version, about = "Declarative user-space state")]
pub struct Cli {
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available subcommands.
///
/// Invariants: the command set holds exactly these two; `apply` and
/// `explain` belong to later layers.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Evaluate the profile, diff against previous state, write the plan.
    Plan(PlanArgs),
    /// Evaluate the profile and diff, computing the summary in memory.
    Status(StatusArgs),
}

/// Arguments for `confit plan`.
///
/// Yields the evaluated plan plus its summary.
/// Guarantees: `profile` is required; omitted `output` prints the plan to
/// stdout; omitted `root` resolves to the profile file's parent directory
/// (a profile path without a parent rejects with a config error); omitted
/// `state` resolves to the empty previous directly; `format` defaults to
/// JSON; `conflicts` defaults off, showing winner tools only, and expands
/// changed lines to winner-over-loser while set.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Profile Lua file to evaluate.
    #[arg(long)]
    pub profile: PathBuf,
    /// Confit project root for `require` resolution; defaults to the profile
    /// file's parent directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Destination path for the exported plan; omitted prints to stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Previous state file; omitted resolves to the empty previous directly.
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Plan export format.
    #[arg(long, default_value = "json", value_parser = PlanFormat::parse)]
    pub format: PlanFormat,
    /// Show winner-over-loser attribution on changed lines.
    #[arg(long, default_value_t = false)]
    pub conflicts: bool,
}

/// Arguments for `confit status`.
///
/// Yields the summary in memory without writing a plan file.
/// Guarantees: mirrors [`PlanArgs`] minus `--output`/`--format` (status
/// produces the summary in memory, with output path and serialization choice
/// belonging to `plan`); omitted `root` resolves to the profile file's parent
/// directory (a profile path without a parent rejects with a config error);
/// omitted `state` resolves to the empty previous directly; `conflicts`
/// defaults off, showing winner tools only.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Profile Lua file to evaluate.
    #[arg(long)]
    pub profile: PathBuf,
    /// Confit project root for `require` resolution; defaults to the profile
    /// file's parent directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Previous state file; omitted resolves to the empty previous directly.
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Show winner-over-loser attribution on changed lines.
    #[arg(long, default_value_t = false)]
    pub conflicts: bool,
}

/// Resolve the project root: explicit flag wins, else profile parent.
///
/// Yields the flag value while present; otherwise the profile file's parent
/// directory. Rejects profiles without a usable parent with a config error.
///
/// Args: `root` is the optional flag, `profile` the profile file path.
fn resolve_root(root: &Option<PathBuf>, profile: &std::path::Path) -> Result<PathBuf> {
    if let Some(root) = root {
        return Ok(root.clone());
    }
    match profile.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => Err(Error::Config(format!(
            "profile '{}' has no parent directory: pass --root explicitly",
            profile.display()
        ))),
    }
}

/// Evaluated plan plus its terminal summary.
///
/// Carries both outputs of `plan` so the caller decides placement: plan
/// payload to the `--output` path or stdout, summary to stderr. Args: `plan`
/// is the merged plan, `summary` its rendered terminal text.
#[derive(Debug)]
pub struct PlanOutcome {
    /// Merged plan, ready to serialize.
    pub plan: Plan,
    /// Rendered terminal summary.
    pub summary: String,
}

/// Run `plan`: evaluate, diff, return plan plus summary.
///
/// Loads the previous state through `store`, orchestrates the plan, diffs
/// entry by entry, summarizes (rich by default, winner-over-loser under
/// `--conflicts`). Publishing (file or stdout) belongs to the caller.
/// Args: `args` are the parsed CLI flags, `store` the previous-state source.
///
/// Example:
/// ```rust,no_run
/// use std::path::PathBuf;
/// use confit::cli::{PlanArgs, run_plan};
/// use confit::store::{FsStateStore, PlanFormat};
///
/// let args = PlanArgs {
///     profile: PathBuf::from("examples/0-basic_tool/profile.lua"),
///     root: Some(PathBuf::from("examples/0-basic_tool")),
///     output: None,
///     state: None,
///     format: PlanFormat::Json,
///     conflicts: false,
/// };
/// let outcome = run_plan(&args, &FsStateStore::new(None)).unwrap();
/// assert!(outcome.summary.contains("plan:"));
/// ```
pub fn run_plan(args: &PlanArgs, store: &dyn StateStore) -> Result<PlanOutcome> {
    let root = resolve_root(&args.root, &args.profile)?;
    let graph = evaluate(&root, &args.profile)?;
    let previous = store.load()?;
    let plan = crate::plan::orchestrate(
        &graph,
        &previous,
        &root.display().to_string(),
        &args.profile.display().to_string(),
    )?;
    let counts = crate::plan::diff(&plan, &previous);
    let details = crate::diff::detail(&plan, &previous);
    let summary = crate::plan::summarize(&plan, &counts, &details, args.conflicts);
    Ok(PlanOutcome { plan, summary })
}

/// Run `status`: evaluate, diff, return the summary, computing in memory.
///
/// Same as [`run_plan`] minus the publish step: evaluation and diffing run
/// in memory, keeping reads within the evaluation root. Args: `args` are the
/// parsed CLI flags, `store` the previous-state source.
pub fn run_status(args: &StatusArgs, store: &dyn StateStore) -> Result<String> {
    let root = resolve_root(&args.root, &args.profile)?;
    let graph = evaluate(&root, &args.profile)?;
    let previous = store.load()?;
    let plan = crate::plan::orchestrate(
        &graph,
        &previous,
        &root.display().to_string(),
        &args.profile.display().to_string(),
    )?;
    let counts = crate::plan::diff(&plan, &previous);
    let details = crate::diff::detail(&plan, &previous);
    Ok(crate::plan::summarize(
        &plan,
        &counts,
        &details,
        args.conflicts,
    ))
}
