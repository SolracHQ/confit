//! Plan run
//!
//! Profile flags into built plans with drift.

use std::path::{Path, PathBuf};

use confit_core::drift::Drift;
use confit_core::error::Result;
use confit_core::fs::{snapshot, snapshot_tree};

use crate::fs::OsFs;
use confit_core::ids::DocPath;
use confit_core::plan::Plan;
use confit_core::runtime::Runtime;
use confit_core::store::{load_state, write_plan};

use crate::cli::PlanArgs;

use super::seams::{evaluate_shared, log_processed, timed};

use confit_engine::{ProgressCallback, ProgressEvent};

/// Outcome of one profile run with its previous plan.
#[derive(Debug)]
pub struct PlanOutcome {
    /// Holds the built plan with counts.
    pub built: Plan,
    /// Holds the previous plan backing lifecycle marks.
    pub previous: Plan,
    /// Holds plan versus disk edits leading the summary.
    pub drift: Vec<Drift>,
    /// Holds hook preview lines beside the summary.
    pub hook_lines: Vec<String>,
    /// Holds the tmp plan path while no output destination passes.
    pub stored: Option<PathBuf>,
}

/// One plan run from plan flags to a built plan.
///
/// # Examples
///
/// ```text,no_run
/// use confit_cli::actions::plan::PlanRunner;
/// use confit_cli::cli::{PlanArgs, SharedArgs};
/// use std::path::PathBuf;
///
/// let args = PlanArgs {
///     profile: PathBuf::from("profile.lua"),
///     shared: SharedArgs {
///         root: None,
///         plugins: None,
///         re_fetch: false,
///     },
///     output: None,
/// };
/// let runner = PlanRunner { args: &args, store_tmp: false, progress: None };
/// let outcome = runner.execute();
/// assert!(matches!(outcome, Ok(_) | Err(_)));
/// ```
pub struct PlanRunner<'a> {
    /// Holds the plan flags under running.
    pub args: &'a PlanArgs,
    /// Stores the payload under tmp while no destination passes.
    pub store_tmp: bool,
    /// Gains facts, holding `None` for silence.
    pub progress: Option<ProgressCallback>,
}

impl PlanRunner<'_> {
    /// Emits one hashing fact while a sink passes.
    fn emit_hashing(&self) {
        if let Some(sink) = self.progress.as_ref() {
            sink(ProgressEvent::Hashing);
        }
    }

    /// Emits one plan-reading fact while a sink passes.
    fn emit_reading_plan(&self, path: &Path) {
        if let Some(sink) = self.progress.as_ref() {
            sink(ProgressEvent::ReadingPlan {
                path: path.display().to_string(),
            });
        }
    }

    /// Emits one plan-writing fact while a sink passes.
    fn emit_writing_plan(&self, documents: usize) {
        if let Some(sink) = self.progress.as_ref() {
            sink(ProgressEvent::WritingPlan { documents });
        }
    }

    /// Evaluates the engine, loads previous plan, diffs drift,
    /// builds the core plan, and writes the payload on demand.
    ///
    /// # Returns
    ///
    /// The built plan with its previous plan plus drift.
    ///
    /// # Errors
    ///
    /// Evaluation plus plan plus build plus write failures surface
    /// as plan or io errors.
    pub fn execute(self) -> Result<PlanOutcome> {
        let evaluation =
            evaluate_shared(&self.args.shared, &self.args.profile, self.progress.clone())?;
        let documents = evaluation.documents;
        let state_file = confit_core::store::default_state_path()?;
        self.emit_reading_plan(&state_file);
        let previous = load_state(Some(&state_file), &OsFs)?;
        let fs = OsFs;
        let snapshot = |path: &DocPath| snapshot(path, &fs);
        let snapshot_tree = |path: &DocPath| snapshot_tree(&path.expand(), &fs);
        let drifts = timed("drift", || previous.drift(&snapshot, &snapshot_tree));
        self.emit_hashing();
        let built = timed("hash", || Plan::build(documents, evaluation.hooks))?;
        log_processed(&built, &previous);
        let hook_lines = built.hook_preview(&Runtime::current(), &OsFs)?;
        if self.args.output.is_some() || self.store_tmp {
            self.emit_writing_plan(built.documents.len());
        }
        let stored = timed("write", || {
            match (self.args.output.as_deref(), self.store_tmp) {
                (Some(dest), _) => {
                    let resolved = crate::cli::resolve_plan_file(dest)?;
                    write_plan(&built, Some(&resolved), &OsFs).map(|()| None)
                }
                (None, true) => {
                    let tmp = tmp_plan_path();
                    write_plan(&built, Some(&tmp), &OsFs).map(|()| Some(tmp))
                }
                (None, false) => Ok(None),
            }
        })?;
        Ok(PlanOutcome {
            built,
            previous,
            drift: drifts,
            hook_lines,
            stored,
        })
    }
}

/// Builds one tmp plan path under the OS temp folder.
///
/// The process id separates parallel runs. Tmp cleanup stays
/// the OS job.
///
/// # Returns
///
/// The tmp destination for the plan payload.
///
/// # Examples
///
/// ```text
/// use confit_cli::actions::plan::tmp_plan_path;
///
/// let path = tmp_plan_path();
/// assert!(matches!(path.to_string_lossy().contains("confit-plan-"), true));
/// ```
pub fn tmp_plan_path() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let slot = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("confit-plan-{}-{slot}.json", std::process::id()))
}
