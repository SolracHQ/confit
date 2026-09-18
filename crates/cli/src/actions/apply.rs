//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::path::PathBuf;

use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::{Error, Result};
use confit_core::fs::{Filesystem, snapshot, snapshot_tree};
use confit_core::hook::{describe_condition, resolve_hook};
use confit_core::ids::DocPath;
use confit_core::plan::Bundle;
use confit_core::runtime::{Runtime, evaluate};
use confit_core::store::{
    archive_previous, default_state_path, load_bundle_input, load_state, prune_blobs,
    remove_orphans, remove_tree_members, resolve_named_plan, resolve_previous_dir, stored_entries,
    write_documents, write_manifest,
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
    /// Holds the stored plan path backing apply of the past.
    pub stored: PathBuf,
}

/// One apply run from desired documents to disk writes.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::apply::ApplyRunner;
/// use confit_cli::actions::seams::Seams;
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::plan::Bundle;
/// use std::io::Cursor;
/// use std::path::Path;
///
/// let fs = MemoryFs::new();
/// let mut input = Cursor::new("yes\n");
/// let mut output = Vec::new();
/// let plan = match Bundle::build(
///     vec![ManifestDocument::new(
///         DocPath::new("note"),
///         ManifestData::Text { content: "hi".into(), mode: None },
///     )],
///     Vec::new(),
/// ) {
///     Ok(plan) => plan,
///     Err(error) => panic!("plan builds: {error}"),
/// };
/// let runner = ApplyRunner {
///     plan,
///     previous: Bundle::empty(),
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
    pub plan: Bundle,
    /// Holds the previous plan backing drift plus counts.
    pub previous: Bundle,
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
    /// The positional sniffs its shape: `@name` reads a named
    /// slot, `%N` reads history newest-first from one, `.cb`
    /// reads a bundle file, everything else evaluates as
    /// a profile.
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
    ///     source: PathBuf::from("profile.lua"),
    ///     shared: confit_cli::cli::SharedArgs {
    ///         root: None,
    ///         plugins: None,
    ///         re_fetch: false,
    ///     },
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
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("");
        if let Some(name) = raw.strip_prefix('@') {
            let slot_plan = load_named_slot(name, seams.fs)?;
            return Self::from_slot(slot_plan, args.force, seams);
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick = parse_history_pick(raw, rest)?;
            let slot_plan = load_history_pick(raw, pick, seams.fs)?;
            return Self::from_slot(slot_plan, args.force, seams);
        }
        if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            seams.emit_reading_plan(positional);
            let file_plan = timed("apply plan load", || {
                load_bundle_input(positional, seams.fs)
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
        if !seams.fs.exists(positional) {
            return Err(Error::Plan(format!(
                "apply reads no profile '{}'",
                positional.display()
            )));
        }
        let evaluation = evaluate_shared(&args.shared, positional, seams.progress.clone())?;
        let state_file = default_state_path()?;
        seams.emit_reading_plan(&state_file);
        let previous = load_state(Some(state_file.as_path()), seams.fs)?;
        let mut plan = Bundle::build(evaluation.documents, evaluation.hooks)?;
        plan.blobs = evaluation.blobs;
        Ok(Self {
            plan,
            previous,
            state: Some(state_file),
            force: args.force,
            preview: true,
            seams,
        })
    }

    /// Builds a slot-backed runner with preview plus prompts.
    fn from_slot(slot_plan: Bundle, force: bool, seams: Seams<'a>) -> Result<Self> {
        let state_file = default_state_path()?;
        seams.emit_reading_plan(&state_file);
        let previous = load_state(Some(state_file.as_path()), seams.fs)?;
        Ok(Self {
            plan: slot_plan,
            previous,
            state: Some(state_file),
            force,
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
    ///     source: PathBuf::from("profile.lua"),
    ///     shared: confit_cli::cli::SharedArgs {
    ///         root: None,
    ///         plugins: None,
    ///         re_fetch: false,
    ///     },
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
        let built = std::mem::replace(&mut self.plan, Bundle::empty());
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
        write_documents(&built.manifest.documents, &built.blobs, fs, notify)?;
        let removed = remove_orphans(
            &self.previous.manifest.documents,
            &built.manifest.documents,
            fs,
        )?;
        let removed = removed
            + remove_tree_members(
                &self.previous.manifest.documents,
                &built.manifest.documents,
                fs,
            )?;
        if self.state.is_some() {
            self.seams.emit_writing_plan(built.manifest.documents.len());
        }
        if let Some(state) = self.state.as_deref() {
            write_manifest(&built, Some(state), fs)?;
        }
        self.seams.emit_writing_plan(built.manifest.documents.len());
        let stored = archive_previous(&built, fs)?;
        prune_blobs(fs)?;
        self.run_hooks(&built, &rt, fs)?;
        Ok(ApplyReport {
            written: built.manifest.documents.len(),
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
    fn run_hooks(&mut self, built: &Bundle, rt: &Runtime, fs: &dyn Filesystem) -> Result<()> {
        let total = built.manifest.hooks.len();
        let real = OsRunner;
        let runner: &dyn HookRunner = match self.seams.hook_runner {
            Some(runner) => runner,
            None => &real,
        };
        for (index, hook) in built.manifest.hooks.iter().enumerate() {
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

/// Parses one `%N` history pick.
///
/// # Arguments
///
/// * `raw` - the full picker text for error context.
/// * `rest` - the digits after the `%` sigil.
///
/// # Returns
///
/// The 1-based pick newest-first from one.
///
/// # Errors
///
/// Non-numeric picks fail as plan errors.
fn parse_history_pick(raw: &str, rest: &str) -> Result<usize> {
    rest.parse().map_err(|_| {
        Error::Plan(format!(
            "apply: '{raw}' reads unsupported, want '%N' holding a number from 1"
        ))
    })
}

/// Loads one history entry newest-first from one.
///
/// # Arguments
///
/// * `raw` - the full picker text for error context.
/// * `pick` - the 1-based pick newest-first from one.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes.
///
/// # Errors
///
/// Out-of-range picks fail naming the stored count. Load
/// failures surface as plan or io errors.
fn load_history_pick(raw: &str, pick: usize, fs: &dyn Filesystem) -> Result<Bundle> {
    let dir = resolve_previous_dir()?;
    let entries = stored_entries(&dir, fs)?;
    let total = entries.len();
    if pick < 1 || pick > total {
        return Err(Error::Plan(format!(
            "apply: '{raw}' reads out of range, holding {total} stored plans"
        )));
    }
    entries
        .into_iter()
        .nth(pick - 1)
        .map(|(_, plan)| plan)
        .ok_or_else(|| {
            Error::Plan(format!(
                "apply: '{raw}' reads out of range, holding {total} stored plans"
            ))
        })
}

/// Loads one named slot plan.
///
/// # Arguments
///
/// * `name` - the slot name without the `@` sigil.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes.
///
/// # Errors
///
/// Absent slots plus load failures surface as plan or io errors.
fn load_named_slot(name: &str, fs: &dyn Filesystem) -> Result<Bundle> {
    let path = resolve_named_plan(name)?;
    if !fs.exists(&path) {
        return Err(Error::Plan(format!("apply: '@{name}' reads absent")));
    }
    load_state(Some(path.as_path()), fs)
}
