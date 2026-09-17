//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::path::PathBuf;

use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::{Error, Result};
use confit_core::fs::{Filesystem, snapshot, snapshot_tree};
use confit_core::hook::{describe_condition, resolve_hook};
use confit_core::ids::DocPath;
use confit_core::plan::Plan;
use confit_core::runtime::{Runtime, evaluate};
use confit_core::store::{
    archive_previous, default_state_path, load_state, remove_orphans, remove_tree_members,
    write_documents, write_plan,
};

use crate::actions::hooks::{HookRunner, OsRunner, append_hook_log};
use crate::cli::ApplyArgs;
use crate::presentation::summary::Summary;

use super::seams::{Seams, evaluate_shared, log_processed, timed};

use confit_engine::ProgressEvent;

/// Outcome of one successful apply run.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::apply::ApplyReport;
/// use std::path::PathBuf;
///
/// let report = ApplyReport { written: 1, removed: 0, stored: PathBuf::from("previous/x.json") };
/// assert!(matches!((report.written, report.removed), (1, 0)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Counts documents written to disk.
    pub written: usize,
    /// Counts recorded orphans plus dropped tree members removed from disk.
    pub removed: usize,
    /// Holds the stored plan path backing recover.
    pub stored: PathBuf,
}

/// One apply run from desired documents to disk writes.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::apply::ApplyRunner;
/// use confit_cli::actions::seams::Seams;
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::plan::Plan;
/// use std::io::Cursor;
/// use std::path::Path;
///
/// let fs = MemoryFs::new();
/// let mut input = Cursor::new("yes\n");
/// let mut output = Vec::new();
/// let plan = match Plan::build(
///     vec![Document::new(
///         DocPath::new("note"),
///         DocumentData::Text { content: "hi".into(), mode: None },
///     )],
///     Vec::new(),
/// ) {
///     Ok(plan) => plan,
///     Err(error) => panic!("plan builds: {error}"),
/// };
/// let runner = ApplyRunner {
///     plan,
///     previous: Plan::empty(),
///     state: None,
///     force: false,
///     preview: false,
///     seams: Seams::memory(&fs, &mut input, &mut output),
/// };
/// assert!(matches!(runner.execute(), Ok(_)));
/// assert!(fs.exists(Path::new("note")));
/// ```
pub struct ApplyRunner<'a> {
    /// Holds the desired plan under writing plus running.
    pub plan: Plan,
    /// Holds the previous plan backing drift plus counts.
    pub previous: Plan,
    /// Holds the state file gaining the new plan, `None` skips.
    pub state: Option<PathBuf>,
    /// Skips the first prompt. Drift still re-prompts.
    pub force: bool,
    /// Renders the preview through presentation first.
    pub preview: bool,
    /// Holds the injected filesystem plus prompts plus sink.
    pub seams: Seams<'a>,
}

impl<'a> ApplyRunner<'a> {
    /// Reads desired documents from flags on injected seams.
    ///
    /// A plan file runs on the file alone with no profile
    /// flag plus no engine. Otherwise the profile evaluates
    /// through the engine first.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `seams` - the injected filesystem plus prompts plus sink.
    ///
    /// # Returns
    ///
    /// The runner holding desired documents plus run flags.
    ///
    /// # Errors
    ///
    /// Evaluation plus plan load failures surface as plan
    /// or io errors.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use confit_cli::actions::apply::ApplyRunner;
    /// use confit_cli::actions::seams::Seams;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_cli::fs::OsFs;
    /// use std::io::Cursor;
    /// use std::path::PathBuf;
    ///
    /// let args = ApplyArgs {
    ///     profile: Some(PathBuf::from("profile.lua")),
    ///     shared: confit_cli::cli::SharedArgs {
    ///         root: None,
    ///         plugins: None,
    ///         re_fetch: false,
    ///     },
    ///     plan: None,
    ///     force: true,
    /// };
    /// let fs = OsFs;
    /// let mut input = Cursor::new(String::new());
    /// let mut output = Vec::new();
    /// let seams = Seams::memory(&fs, &mut input, &mut output);
    /// let runner = ApplyRunner::from_args(&args, seams);
    /// assert!(matches!(runner, Ok(_) | Err(_)));
    /// ```
    pub fn from_args(args: &ApplyArgs, seams: Seams<'a>) -> Result<Self> {
        if let Some(plan_file) = args.plan.as_deref() {
            let resolved = crate::cli::resolve_plan_file(plan_file)?;
            seams.emit_reading_plan(&resolved);
            let file_plan = timed("apply plan load", || {
                load_state(Some(resolved.as_path()), seams.fs)
            })?;
            let state_file = default_state_path()?;
            seams.emit_reading_plan(&state_file);
            let previous = load_state(Some(state_file.as_path()), seams.fs)?;
            return Ok(Self {
                plan: file_plan,
                previous,
                state: Some(state_file),
                force: args.force,
                preview: false,
                seams,
            });
        }
        let profile = args
            .profile
            .as_deref()
            .ok_or_else(|| Error::Plan("apply needs --profile while absent".to_string()))?;
        let evaluation = evaluate_shared(&args.shared, profile, seams.progress.clone())?;
        let state_file = default_state_path()?;
        seams.emit_reading_plan(&state_file);
        let previous = load_state(Some(state_file.as_path()), seams.fs)?;
        Ok(Self {
            plan: Plan::build(evaluation.documents, evaluation.hooks)?,
            previous,
            state: Some(state_file),
            force: args.force,
            preview: true,
            seams,
        })
    }

    /// Reads flags plus runs the full apply flow on injected seams.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `seams` - the injected filesystem plus prompts plus sink.
    ///
    /// # Returns
    ///
    /// The write counts plus the stored plan path.
    ///
    /// # Errors
    ///
    /// Evaluation plus prompt plus write failures surface as
    /// plan or io errors. A non-`yes` answer aborts as a plan error.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use confit_cli::actions::apply::ApplyRunner;
    /// use confit_cli::actions::seams::Seams;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_cli::fs::OsFs;
    /// use std::io::Cursor;
    /// use std::path::PathBuf;
    ///
    /// let args = ApplyArgs {
    ///     profile: Some(PathBuf::from("profile.lua")),
    ///     shared: confit_cli::cli::SharedArgs {
    ///         root: None,
    ///         plugins: None,
    ///         re_fetch: false,
    ///     },
    ///     plan: None,
    ///     force: true,
    /// };
    /// let fs = OsFs;
    /// let mut input = Cursor::new(String::new());
    /// let mut output = Vec::new();
    /// let seams = Seams::memory(&fs, &mut input, &mut output);
    /// let report = ApplyRunner::run(&args, seams);
    /// assert!(matches!(report, Ok(_) | Err(_)));
    /// ```
    pub fn run(args: &ApplyArgs, seams: Seams<'a>) -> Result<ApplyReport> {
        Self::from_args(args, seams)?.execute()
    }

    /// Applies desired documents with preview plus prompts plus rotation.
    ///
    /// The preview renders through presentation. Only the literal
    /// `yes` proceeds, anything else aborts with nothing written.
    /// A fresh snapshot before writing re-prompts on drift. Success
    /// writes the state file plus one stored plan with rotation.
    ///
    /// # Returns
    ///
    /// The write counts plus the stored plan path.
    ///
    /// # Errors
    ///
    /// Build plus prompt plus write failures surface as plan or
    /// io errors. A non-`yes` answer aborts as a plan error.
    pub fn execute(mut self) -> Result<ApplyReport> {
        self.seams.emit_hashing();
        let fs: &dyn Filesystem = self.seams.fs;
        let built = std::mem::replace(&mut self.plan, Plan::empty());
        log_processed(&built, &self.previous);
        let snapshot = |path: &DocPath| snapshot(path, fs);
        let snapshot_tree = |path: &DocPath| snapshot_tree(&path.expand(), fs);
        let first_run = match self.state.as_deref() {
            Some(slot) => !fs.exists(slot),
            None => false,
        };
        let reference = if first_run { &built } else { &self.previous };
        let order = if first_run {
            DriftOrder::DiskFirst
        } else {
            DriftOrder::RecordedFirst
        };
        let baseline = reference.drift(&snapshot, &snapshot_tree, order);
        let rt = Runtime::current();
        if self.preview {
            let report = Summary {
                built: &built,
                previous: &self.previous,
                drift: &baseline,
                first_run,
            };
            let text = report.render();
            self.seams
                .output
                .write_all(text.as_bytes())
                .map_err(Error::from)?;
            self.seams.output.write_all(b"\n").map_err(Error::from)?;
            for line in built.hook_preview(&rt, fs)? {
                self.seams
                    .output
                    .write_all(line.as_bytes())
                    .map_err(Error::from)?;
                self.seams.output.write_all(b"\n").map_err(Error::from)?;
            }
        }
        if !self.force && !self.seams.confirm()? {
            return Err(Error::Plan(
                "apply aborted: answer reads no 'yes'".to_string(),
            ));
        }
        let fresh = reference.drift(&snapshot, &snapshot_tree, order);
        if fresh != baseline {
            for line in Drift::lines(&fresh) {
                self.seams
                    .output
                    .write_all(line.as_bytes())
                    .map_err(Error::from)?;
                self.seams.output.write_all(b"\n").map_err(Error::from)?;
            }
            if !self.seams.confirm()? {
                return Err(Error::Plan(
                    "apply aborted: answer reads no 'yes'".to_string(),
                ));
            }
        }
        let notify_written;
        let notify = if let Some(sink) = self.seams.progress.clone() {
            notify_written = move |path: &DocPath| {
                sink(ProgressEvent::DocumentWritten {
                    path: path.as_str().to_string(),
                });
            };
            Some(&notify_written as &dyn Fn(&DocPath))
        } else {
            None
        };
        write_documents(&built.documents, fs, notify)?;
        let removed = remove_orphans(&self.previous.documents, &built.documents, fs)?;
        let removed =
            removed + remove_tree_members(&self.previous.documents, &built.documents, fs)?;
        if self.state.is_some() {
            self.seams.emit_writing_plan(built.documents.len());
        }
        if let Some(state) = self.state.as_deref() {
            write_plan(&built, Some(state), fs)?;
        }
        self.seams.emit_writing_plan(built.documents.len());
        let stored = archive_previous(&built, fs)?;
        self.run_hooks(&built, &rt, fs)?;
        Ok(ApplyReport {
            written: built.documents.len(),
            removed,
            stored,
        })
    }

    /// Runs built hooks after files, state, plus history land.
    ///
    /// Each hook re-resolves, re-gates, skips on passing
    /// pre-checks, spawns through the runner, verifies
    /// post-checks, and appends captured bytes to the run log.
    /// Failures abort the rest.
    fn run_hooks(&mut self, built: &Plan, rt: &Runtime, fs: &dyn Filesystem) -> Result<()> {
        let total = built.hooks.len();
        let real = OsRunner;
        let runner: &dyn HookRunner = match self.seams.hook_runner {
            Some(runner) => runner,
            None => &real,
        };
        for (index, hook) in built.hooks.iter().enumerate() {
            let position = index + 1;
            let argv_text = hook.argv.join(" ");
            if let Some(gate) = hook.when.as_ref()
                && !evaluate(gate, rt, fs)
            {
                let line = format!(
                    "warn: {argv_text} cannot run ({})",
                    describe_condition(gate)
                );
                self.seams
                    .output
                    .write_all(line.as_bytes())
                    .map_err(Error::from)?;
                self.seams.output.write_all(b"\n").map_err(Error::from)?;
                continue;
            }
            if !hook.checks.is_empty() && hook.checks.iter().all(|check| evaluate(check, rt, fs)) {
                let line = format!("skipped: {argv_text} (checks pass)");
                self.seams
                    .output
                    .write_all(line.as_bytes())
                    .map_err(Error::from)?;
                self.seams.output.write_all(b"\n").map_err(Error::from)?;
                continue;
            }
            let binary = resolve_hook(hook, rt, fs).ok_or_else(|| {
                let head = hook.argv.first().cloned().unwrap_or_default();
                Error::Plan(format!("hook '{argv_text}' cannot resolve '{head}'"))
            })?;
            let mut spawn: Vec<String> = vec![binary.display().to_string()];
            spawn.extend(hook.argv.iter().skip(1).cloned());
            let line = format!("hook {position} of {total}: {argv_text}");
            self.seams
                .output
                .write_all(line.as_bytes())
                .map_err(Error::from)?;
            self.seams.output.write_all(b"\n").map_err(Error::from)?;
            if let Some(sink) = self.seams.progress.as_ref() {
                sink(ProgressEvent::HookRunning {
                    position,
                    total,
                    argv: argv_text.clone(),
                });
            }
            let path_dirs: Vec<std::path::PathBuf> =
                hook.path.iter().map(std::path::PathBuf::from).collect();
            let outcome = runner.run(&spawn, &path_dirs, hook.timeout_secs)?;
            if let Some(log) = self.seams.log_file.clone() {
                append_hook_log(fs, &log, &line, &outcome.output)?;
            }
            if outcome.code != 0 {
                return Err(Error::Plan(format!(
                    "hook '{argv_text}' failed with code {}",
                    outcome.code
                )));
            }
            if !hook.checks.is_empty() {
                let failed: Vec<String> = hook
                    .checks
                    .iter()
                    .filter(|check| !evaluate(check, rt, fs))
                    .map(describe_condition)
                    .collect();
                if !failed.is_empty() {
                    log::warn!(
                        "hook '{argv_text}' failed checks after run: {}",
                        failed.join(", ")
                    );
                    return Err(Error::Plan(format!(
                        "hook '{argv_text}' failed checks after run: {}",
                        failed.join(", ")
                    )));
                }
            }
            log::debug!("hook {position} of {total} ran code={}", outcome.code);
        }
        Ok(())
    }
}
