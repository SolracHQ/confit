//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::collections::BTreeSet;
use std::io::BufRead;
use std::path::PathBuf;

use confit_driver as driver;
use confit_model::arg::Arg;
use confit_model::drift::{Drift, DriftOrder};
use confit_model::error::{Error, Result};
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use confit_runtime::Applier;
use confit_runtime::Checks;
use confit_store::Stores;

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
    /// Evaluation and manifest load failures surface as plan
    /// or io errors.
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
            let (slot_manifest, _) = stores
                .slots()
                .resolve(Some(raw))
                .map_err(|error| Error::Plan(error.to_string()))?;
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
            let file_manifest = timed("apply plan load", || {
                stores
                    .bundles()
                    .read(positional)
                    .map_err(|error| Error::Plan(error.to_string()))
            })?;
            let previous = stores
                .slots()
                .load()
                .map_err(|error| Error::Plan(error.to_string()))?;
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
            return Err(Error::Plan(format!(
                "apply reads no profile '{}'",
                positional.display()
            )));
        }
        let evaluation = evaluate_shared(
            &args.shared,
            positional,
            Some(sinks.progress.clone()),
            &stores,
        )?;
        let previous = stores
            .slots()
            .load()
            .map_err(|error| Error::Plan(error.to_string()))?;
        let manifest = Manifest::build(evaluation.documents, evaluation.hooks)
            .map_err(|error| Error::Plan(error.to_string()))?;
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
        let previous = stores
            .slots()
            .load()
            .map_err(|error| Error::Plan(error.to_string()))?;
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
    /// Build, prompt, and write failures surface as plan or
    /// io errors. A non-`yes` answer aborts as a plan error.
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
            return Err(Error::Plan(
                "apply aborted: answer reads no 'yes'".to_string(),
            ));
        }
        let fresh = self.applier.drift(reference, order);
        if fresh != baseline {
            for line in drift_lines(&fresh) {
                self.sinks.print_line(line);
            }
            if !self.sinks.confirm(self.input)? {
                return Err(Error::Plan(
                    "apply aborted: answer reads no 'yes'".to_string(),
                ));
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
        let _ = self
            .stores
            .slots()
            .store(&built)
            .map_err(|error| Error::Plan(error.to_string()))?;
        let mut handles = Vec::new();
        for document in &built.documents {
            for blob in document.data.blob_refs() {
                handles.push(
                    self.stores
                        .blobs()
                        .resolve(blob)
                        .map_err(|error| Error::Plan(error.to_string()))?,
                );
            }
        }
        self.stores
            .blobs()
            .persist(&handles)
            .map_err(|error| Error::Plan(error.to_string()))?;
        self.stores
            .blobs()
            .prune()
            .map_err(|error| Error::Plan(error.to_string()))?;
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
    /// Unresolvable binaries, nonzero codes, and unmet
    /// post-checks fail as plan errors.
    fn spawn_hook(
        &mut self,
        hook: &confit_model::hook::Hook,
        position: usize,
        total: usize,
    ) -> Result<()> {
        let argv_text = Arg::join(&hook.argv);
        let binary = resolve_hook(hook, &self.checks).ok_or_else(|| {
            let head = hook.argv.first().map(Arg::display).unwrap_or_default();
            Error::Plan(format!("hook '{argv_text}' cannot resolve '{head}'"))
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
            return Err(Error::Plan(format!(
                "hook '{argv_text}' failed with code {}",
                outcome.code
            )));
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
    /// Unmet post-checks fail as plan errors naming the hook.
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
        log::warn!(
            "hook '{argv_text}' failed checks after run: {}",
            failed.join(", ")
        );
        Err(Error::Plan(format!(
            "hook '{argv_text}' failed checks after run: {}",
            failed.join(", ")
        )))
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
