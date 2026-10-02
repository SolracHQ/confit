//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::PathBuf;

use confit_driver as driver;
use confit_model::arg::Arg;
use confit_model::drift::{Drift, DriftOrder};
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use confit_runtime::Applier;
use confit_runtime::Checks;
use confit_store::Stores;

use crate::error::{CliError, Result};

use crate::cli::ApplyArgs;
use crate::hooks::{self, append_hook_log};
use crate::presentation::drift::drift_lines;
use crate::presentation::hooks::{describe_condition, evaluate_hooks, resolve_hook};
use crate::presentation::summary::{Hooks, Summary};

use crate::seams::{Sinks, evaluate_shared, log_processed, timed};

use confit_model::progress::Event;

/// Outcome of one successful apply run.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Counts documents written to disk.
    pub written: usize,
    /// Counts recorded orphans and dropped tree members removed from disk.
    pub removed: usize,
}

/// One apply run from desired documents to disk writes.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::apply::ApplyRunner;
/// use confit_model::document::{Data, Document};
/// use confit_model::routes::{Route, RouteBase};
/// use confit_model::manifest::Manifest;
/// use confit_runtime::Applier;
/// use confit_store::{StoreRoots, Stores};
/// use std::io::Cursor;
///
/// let mut input = Cursor::new("yes\n");
/// let (sender, _) = crossbeam_channel::unbounded();
/// let stores = Stores::new(StoreRoots::standard(), sender.clone());
/// let applier = Applier::with_stores(stores.clone(), sender);
/// let manifest = match Manifest::build(
///     vec![Document::new(
///         Route::new(RouteBase::Home, "note").unwrap(),
///         Data::Text { content: "hi".into(), mode: None, unmanaged: false},
///     )],
///     Vec::new(),
/// ) {
///     Ok(manifest) => manifest,
///     Err(error) => panic!("bundle builds: {error}"),
/// };
/// let runner = ApplyRunner {
///     manifest,
///     previous: Manifest::empty(),
///     force: false,
///     input: &mut input,
///     stores,
///     applier,
///     sinks: Default::default(),
///     log_file: None,
///     checks: Default::default(),
///     changed: Default::default(),
/// };
/// assert!(matches!(runner.execute(), Ok(_) | Err(_)));
/// ```
pub struct ApplyRunner<'a> {
    /// Holds the desired manifest under writing and running.
    pub manifest: Manifest,
    /// Holds the previous manifest backing drift and counts.
    pub previous: Manifest,
    /// Skips the first prompt. Drift still re-prompts.
    pub force: bool,
    /// Gains the confirmation answer, stdin on the host.
    pub input: &'a mut dyn BufRead,
    /// Holds the write capabilities for the run.
    pub stores: Stores,
    /// Holds the destination reads and writes for the run.
    pub applier: Applier,
    /// Holds the output senders for the run.
    pub sinks: Sinks,
    /// Gains hook output bytes, holding `None` for no log.
    pub log_file: Option<PathBuf>,
    /// Holds the check facts, populated at execute start.
    pub checks: Checks,
    /// Holds the changed routes, populated at execute start.
    pub changed: BTreeSet<Route>,
}

impl<'a> ApplyRunner<'a> {
    /// Reads desired documents from flags with explicit run fields.
    ///
    /// The positional sniffs its shape: `@name` reads a named
    /// slot, `%N` reads history newest-first from one, `.cb`
    /// reads a bundle file, everything else evaluates as
    /// a profile.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `input` - the answer source under prompting.
    /// * `stores` - the write capabilities for the run.
    /// * `applier` - the destination reads and writes for the run.
    /// * `sinks` - the output senders for the run.
    /// * `log_file` - the hook log path, `None` for no log.
    ///
    /// # Returns
    ///
    /// The runner holding desired documents and run flags.
    ///
    /// # Errors
    ///
    /// - [`CliError::Slot`] for slot reads.
    /// - [`CliError::Bundle`] for bundle reads.
    /// - [`CliError::Model`] for manifest builds.
    /// - [`CliError::Engine`] for evaluation failures.
    /// - [`CliError::NoProfile`] for absent profiles.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use confit_cli::actions::apply::ApplyRunner;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_runtime::Applier;
    /// use confit_store::{StoreRoots, Stores};
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
    /// let mut input = Cursor::new(String::new());
    /// let (sender, _) = crossbeam_channel::unbounded();
    /// let stores = Stores::new(StoreRoots::standard(), sender.clone());
    /// let applier = Applier::with_stores(stores.clone(), sender);
    /// let runner = ApplyRunner::from_args(&args, &mut input, stores, applier, Default::default(), None);
    /// assert!(matches!(runner, Ok(_) | Err(_)));
    /// ```
    pub fn from_args(
        args: &ApplyArgs,
        input: &'a mut dyn BufRead,
        stores: Stores,
        applier: Applier,
        sinks: Sinks,
        log_file: Option<PathBuf>,
    ) -> Result<Self> {
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("");
        if raw.starts_with('@') || raw.starts_with('%') {
            let (slot_manifest, _) = stores.slots().resolve(Some(raw))?;
            return Self::from_slot(
                slot_manifest,
                args.force,
                input,
                stores,
                applier,
                sinks,
                log_file,
            );
        }
        if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            sinks.emit_reading_plan(positional);
            let file_manifest = timed("apply plan load", || stores.bundles().read(positional))?;
            let previous = stores.slots().load()?;
            return Ok(Self {
                manifest: file_manifest,
                previous,
                force: args.force,
                input,
                stores,
                applier,
                sinks,
                log_file,
                checks: Checks::default(),
                changed: BTreeSet::new(),
            });
        }
        if !driver::fs::exists(positional) {
            return Err(CliError::NoProfile {
                path: positional.to_path_buf(),
            });
        }
        let evaluation = evaluate_shared(
            &args.shared,
            positional,
            Some(sinks.progress.clone()),
            &stores,
        )?;
        let previous = stores.slots().load()?;
        let manifest = Manifest::build(evaluation.documents, evaluation.hooks)?;
        Ok(Self {
            manifest,
            previous,
            force: args.force,
            input,
            stores,
            applier,
            sinks,
            log_file,
            checks: Checks::default(),
            changed: BTreeSet::new(),
        })
    }

    /// Builds a slot-backed runner with preview and prompts.
    fn from_slot(
        slot_manifest: Manifest,
        force: bool,
        input: &'a mut dyn BufRead,
        stores: Stores,
        applier: Applier,
        sinks: Sinks,
        log_file: Option<PathBuf>,
    ) -> Result<Self> {
        let previous = stores.slots().load()?;
        Ok(Self {
            manifest: slot_manifest,
            previous,
            force,
            input,
            stores,
            applier,
            sinks,
            log_file,
            checks: Checks::default(),
            changed: BTreeSet::new(),
        })
    }

    /// Reads flags and runs the full apply flow with explicit run fields.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `input` - the answer source under prompting.
    /// * `stores` - the write capabilities for the run.
    /// * `applier` - the destination reads and writes for the run.
    /// * `sinks` - the output senders for the run.
    /// * `log_file` - the hook log path, `None` for no log.
    ///
    /// # Returns
    ///
    /// The write counts and the stored manifest path.
    ///
    /// # Errors
    ///
    /// Evaluation, prompt, and write failures surface as
    /// plan or io errors. A non-`yes` answer aborts as a plan error.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use confit_cli::actions::apply::ApplyRunner;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_runtime::Applier;
    /// use confit_store::{StoreRoots, Stores};
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
    /// let mut input = Cursor::new(String::new());
    /// let (sender, _) = crossbeam_channel::unbounded();
    /// let stores = Stores::new(StoreRoots::standard(), sender.clone());
    /// let applier = Applier::with_stores(stores.clone(), sender);
    /// let report = ApplyRunner::run(&args, &mut input, stores, applier, Default::default(), None);
    /// assert!(matches!(report, Ok(_) | Err(_)));
    /// ```
    pub fn run(
        args: &ApplyArgs,
        input: &'a mut dyn BufRead,
        stores: Stores,
        applier: Applier,
        sinks: Sinks,
        log_file: Option<PathBuf>,
    ) -> Result<ApplyReport> {
        Self::from_args(args, input, stores, applier, sinks, log_file)?.execute()
    }

    /// Applies desired documents with preview, prompts, and rotation.
    ///
    /// The preview renders through presentation. Only the literal
    /// `yes` proceeds, anything else aborts with nothing written.
    /// A fresh snapshot before writing re-prompts on drift. Success
    /// writes the state file and one stored manifest with rotation.
    ///
    /// # Returns
    ///
    /// The write counts and the stored manifest path.
    ///
    /// # Errors
    ///
    /// - [`CliError::Aborted`] for non-`yes` answers.
    /// - [`CliError::Unknown`] for prompt failures.
    /// - [`CliError::Runtime`] for destination writes and removals.
    /// - [`CliError::Slot`] for state stores.
    /// - [`CliError::Blob`] for blob resolves with persists and prunes.
    /// - [`CliError::Resolve`] for unresolvable hooks.
    pub fn execute(mut self) -> Result<ApplyReport> {
        self.sinks.emit_hashing();
        let built = std::mem::replace(&mut self.manifest, Manifest::empty());
        log_processed(&built, &self.previous);
        let first_run = self.stores.slots().is_first_run();
        let reference = if first_run { &built } else { &self.previous };
        let order = if first_run {
            DriftOrder::DiskFirst
        } else {
            DriftOrder::RecordedFirst
        };
        let baseline = self.applier.drift(reference, order);
        self.checks = Checks::current();
        self.changed = changed_paths(&built, &self.previous, &baseline, first_run);
        let evaluated = evaluate_hooks(&built, &self.checks, &self.changed)?;
        let lifecycle = confit_model::hook::diff_lifecycle(&built.hooks, &self.previous.hooks);
        let report = Summary {
            built: &built,
            previous: &self.previous,
            drift: &baseline,
            first_run,
            hooks: Hooks {
                lifecycle: &lifecycle,
                evaluated: &evaluated,
            },
        };
        let text = report.render();
        self.sinks.print_line(text);
        if !self.force && !self.sinks.confirm(self.input)? {
            return Err(CliError::Aborted);
        }
        let fresh = self.applier.drift(reference, order);
        if fresh != baseline {
            for line in drift_lines(&fresh) {
                self.sinks.print_line(line);
            }
            if !self.sinks.confirm(self.input)? {
                return Err(CliError::Aborted);
            }
        }
        let written = self
            .applier
            .write_documents(&built.documents, &self.changed)?;
        let removed = self
            .applier
            .remove_orphans(&self.previous.documents, &built.documents)?;
        let removed = removed
            + self
                .applier
                .remove_tree_members(&self.previous.documents, &built.documents)?;
        self.sinks.emit_writing_manifest(built.documents.len());
        let _ = self.stores.slots().store(&built)?;
        let mut handles = Vec::new();
        for document in &built.documents {
            for blob in document.data.blob_refs() {
                handles.push(self.stores.blobs().resolve(blob)?);
            }
        }
        self.stores.blobs().persist(&handles)?;
        self.stores.blobs().prune()?;
        self.run_hooks(&built)?;
        Ok(ApplyReport { written, removed })
    }

    /// Runs built hooks after files, state, and history land.
    ///
    /// Closed gates skip with a line, passing checks skip silently.
    /// Failures abort the rest.
    fn run_hooks(&mut self, built: &Manifest) -> Result<()> {
        let total = built.hooks.len();
        for (index, hook) in built.hooks.iter().enumerate() {
            let position = index + 1;
            if let Some(line) = self.gate_line(hook) {
                self.sinks.print_line(line);
                continue;
            }
            if !hook.checks.is_empty()
                && hook
                    .checks
                    .iter()
                    .all(|check| self.checks.check(check, &self.changed))
            {
                let line = format!("skipped: {} (checks pass)", Arg::join(&hook.argv));
                self.sinks.print_line(line);
                continue;
            }
            self.spawn_hook(hook, position, total)?;
        }
        Ok(())
    }

    /// Runs one open hook through the registry runner.
    ///
    /// # Errors
    ///
    /// - [`CliError::Resolve`] for unresolvable binaries.
    /// - [`CliError::HookFailed`] for nonzero codes.
    /// - [`CliError::HookChecks`] for unmet post-checks.
    fn spawn_hook(
        &mut self,
        hook: &confit_model::hook::Hook,
        position: usize,
        total: usize,
    ) -> Result<()> {
        let argv_text = Arg::join(&hook.argv);
        let binary = resolve_hook(hook, &self.checks).ok_or_else(|| {
            let head = hook.argv.first().map(Arg::display).unwrap_or_default();
            CliError::Resolve {
                argv: argv_text.clone(),
                head,
            }
        })?;
        let mut spawn: Vec<String> = vec![binary.display().to_string()];
        for slot in hook.argv.iter().skip(1) {
            spawn.push(slot.materialize().to_string_lossy().into_owned());
        }
        let line = format!("hook {position} of {total}: {argv_text}");
        self.sinks.print_line(line.clone());
        let _ = self.sinks.progress.send(Event::HookRunning {
            position,
            total,
            argv: argv_text.clone(),
        });
        let mut path_dirs: Vec<std::path::PathBuf> = Vec::with_capacity(hook.path.len());
        for slot in &hook.path {
            path_dirs.push(slot.materialize());
        }
        let outcome = hooks::run(&spawn, &path_dirs, hook.timeout_secs)?;
        if let Some(log) = self.log_file.clone() {
            append_hook_log(&log, &line, &outcome.output)?;
        }
        if outcome.code != 0 {
            return Err(CliError::HookFailed {
                argv: argv_text.clone(),
                code: outcome.code,
            });
        }
        self.verify_post_checks(hook)?;
        log::debug!("hook {position} of {total} ran code={}", outcome.code);
        Ok(())
    }

    /// Renders the skip line for the first closed gate, else none.
    fn gate_line(&self, hook: &confit_model::hook::Hook) -> Option<String> {
        let argv_text = Arg::join(&hook.argv);
        if let Some(gate) = hook.requires.as_ref()
            && !self.checks.check(gate, &self.changed)
        {
            return Some(format!(
                "warn: {argv_text} cannot run ({})",
                describe_condition(gate)
            ));
        }
        if let Some(gate) = hook.when.as_ref()
            && !self.checks.check(gate, &self.changed)
        {
            return Some(format!(
                "skipped: {argv_text} (no need: {})",
                describe_condition(gate)
            ));
        }
        None
    }

    /// Fails naming post-checks one run leaves unmet.
    ///
    /// # Errors
    ///
    /// - [`CliError::HookChecks`] for unmet post-checks.
    fn verify_post_checks(&self, hook: &confit_model::hook::Hook) -> Result<()> {
        if hook.checks.is_empty() {
            return Ok(());
        }
        let argv_text = Arg::join(&hook.argv);
        let failed: Vec<String> = hook
            .checks
            .iter()
            .filter(|check| !self.checks.check(check, &self.changed))
            .map(describe_condition)
            .collect();
        if failed.is_empty() {
            return Ok(());
        }
        Err(CliError::HookChecks {
            argv: argv_text,
            failed: failed.join(", "),
        })
    }
}

/// Computes the changed destination routes for one apply run.
///
/// First runs hold every desired route. Steady runs hold plan
/// changes and drifted destinations.
fn changed_paths(
    built: &Manifest,
    previous: &Manifest,
    drifts: &[Drift],
    first_run: bool,
) -> BTreeSet<Route> {
    use confit_model::document::DocumentStatus;

    if first_run {
        return built
            .documents
            .iter()
            .map(|document| document.destination.clone())
            .collect();
    }
    let mut out = BTreeSet::new();
    for document in &built.documents {
        if !matches!(document.status(previous), DocumentStatus::Unchanged) {
            out.insert(document.destination.clone());
        }
    }
    for document in &built.documents {
        let prefix = format!("{}/", document.destination.display());
        for drift in drifts {
            let path = drift.path().display();
            if path == document.destination.display() || path.starts_with(&prefix) {
                out.insert(document.destination.clone());
                break;
            }
        }
    }
    out
}
