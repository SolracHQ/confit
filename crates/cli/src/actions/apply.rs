//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::collections::BTreeSet;
use std::path::PathBuf;

use confit_core::arg::Arg;
use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::{Error, Result};
use confit_core::fs::Filesystem;
use confit_core::handles::Route;
use confit_core::plan::Bundle;
use confit_core::probe::PathProbe;
use confit_core::runtime::Runtime;

use crate::cli::ApplyArgs;
use crate::hooks::{HookRunner, OsRunner, append_hook_log};
use crate::presentation::drift::drift_lines;
use crate::presentation::hooks::{describe_condition, evaluate_hooks, resolve_hook};
use crate::presentation::summary::{Hooks, Summary};

use crate::seams::{Seams, evaluate_shared, log_processed, timed};

use confit_core::progress::Event;

/// Outcome of one successful apply run.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Counts documents written to disk.
    pub written: usize,
    /// Counts recorded orphans and dropped tree members removed from disk.
    pub removed: usize,
    /// Holds the stored manifest path backing apply of the past.
    pub stored: PathBuf,
}

/// Shared hook-run context for one apply run.
struct HookCtx<'x> {
    /// Holds the runtime facts under reading.
    rt: &'x Runtime,
    /// Holds the backend under writing logs.
    fs: &'x dyn Filesystem,
    /// Holds the probe under stating.
    probe: &'x dyn PathProbe,
    /// Holds the changed display strings under reading.
    changed: &'x BTreeSet<String>,
}

/// One apply run from desired documents to disk writes.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::apply::ApplyRunner;
/// use confit_cli::seams::Seams;
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::fs::memory::MemoryFs;
/// use confit_core::handles::{Route, RouteBase};
/// use confit_core::plan::Bundle;
/// use confit_core::probe::MemoryProbe;
/// use std::io::Cursor;
///
/// let fs = MemoryFs::new();
/// let probe = MemoryProbe::new();
/// let mut input = Cursor::new("yes\n");
/// let manifest = match Bundle::build(
///     vec![ManifestDocument::new(
///         Route::new(RouteBase::Home, "note").unwrap(),
///         ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
///     )],
///     Vec::new(),
/// ) {
///     Ok(manifest) => manifest,
///     Err(error) => panic!("bundle builds: {error}"),
/// };
/// let runner = ApplyRunner {
///     manifest,
///     previous: Bundle::empty(),
///     force: false,
///     seams: Seams::memory(&fs, &probe, &mut input),
/// };
/// assert!(matches!(runner.execute(), Ok(_) | Err(_)));
/// ```
pub struct ApplyRunner<'a> {
    /// Holds the desired manifest under writing and running.
    pub manifest: Bundle,
    /// Holds the previous manifest backing drift and counts.
    pub previous: Bundle,
    /// Skips the first prompt. Drift still re-prompts.
    pub force: bool,
    /// Holds the injected filesystem, prompts, and sink.
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
    /// * `seams` - the injected filesystem, prompts, and sink.
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
    /// use confit_cli::seams::Seams;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_core::fs::memory::MemoryFs;
    /// use confit_core::probe::MemoryProbe;
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
    /// let fs = MemoryFs::new();
    /// let probe = MemoryProbe::new();
    /// let mut input = Cursor::new(String::new());
    /// let seams = Seams::memory(&fs, &probe, &mut input);
    /// let runner = ApplyRunner::from_args(&args, seams);
    /// assert!(matches!(runner, Ok(_) | Err(_)));
    /// ```
    pub fn from_args(args: &ApplyArgs, seams: Seams<'a>) -> Result<Self> {
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("");
        if raw.starts_with('@') || raw.starts_with('%') {
            let (slot_manifest, _) = seams
                .stores
                .slots()
                .resolve(Some(raw))
                .map_err(prefix_command("apply"))?;
            return Self::from_slot(slot_manifest, args.force, seams);
        }
        if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            seams.emit_reading_plan(positional);
            let file_manifest = timed("apply plan load", || {
                seams.stores.bundles().read(positional)
            })?;
            let previous = seams.stores.slots().load()?;
            return Ok(Self {
                manifest: file_manifest,
                previous,
                force: args.force,
                seams,
            });
        }
        if !seams.fs.exists(positional) {
            return Err(Error::Plan(format!(
                "apply reads no profile '{}'",
                positional.display()
            )));
        }
        let evaluation = evaluate_shared(
            &args.shared,
            positional,
            seams.progress.clone(),
            &seams.stores,
        )?;
        let previous = seams.stores.slots().load()?;
        let mut manifest = Bundle::build(evaluation.documents, evaluation.hooks)?;
        manifest.blobs = evaluation.blobs;
        Ok(Self {
            manifest,
            previous,
            force: args.force,
            seams,
        })
    }

    /// Builds a slot-backed runner with preview and prompts.
    fn from_slot(slot_manifest: Bundle, force: bool, seams: Seams<'a>) -> Result<Self> {
        let previous = seams.stores.slots().load()?;
        Ok(Self {
            manifest: slot_manifest,
            previous,
            force,
            seams,
        })
    }

    /// Reads flags and runs the full apply flow on injected seams.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `seams` - the injected filesystem, prompts, and sink.
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
    /// use confit_cli::seams::Seams;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_core::fs::memory::MemoryFs;
    /// use confit_core::probe::MemoryProbe;
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
    /// let fs = MemoryFs::new();
    /// let probe = MemoryProbe::new();
    /// let mut input = Cursor::new(String::new());
    /// let seams = Seams::memory(&fs, &probe, &mut input);
    /// let report = ApplyRunner::run(&args, seams);
    /// assert!(matches!(report, Ok(_) | Err(_)));
    /// ```
    pub fn run(args: &ApplyArgs, seams: Seams<'a>) -> Result<ApplyReport> {
        Self::from_args(args, seams)?.execute()
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
        self.seams.emit_hashing();
        let built = std::mem::replace(&mut self.manifest, Bundle::empty());
        log_processed(&built, &self.previous);
        let first_run = self.seams.stores.slots().is_first_run();
        let reference = if first_run { &built } else { &self.previous };
        let order = if first_run {
            DriftOrder::DiskFirst
        } else {
            DriftOrder::RecordedFirst
        };
        let baseline = self.seams.applier.drift(reference, order);
        let rt = Runtime::current();
        let probe: &dyn PathProbe = self.seams.probe;
        let changed = changed_paths(&built, &self.previous, &baseline, first_run);
        let changed_ids: BTreeSet<String> = changed.iter().map(|route| route.display()).collect();
        let evaluated = evaluate_hooks(&built, &rt, probe, &changed_ids, &self.seams.applier)?;
        let lifecycle =
            confit_core::hook::diff_lifecycle(&built.manifest.hooks, &self.previous.manifest.hooks);
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
        self.seams.print_line(text);
        if !self.force && !self.seams.confirm()? {
            return Err(Error::Plan(
                "apply aborted: answer reads no 'yes'".to_string(),
            ));
        }
        let fresh = self.seams.applier.drift(reference, order);
        if fresh != baseline {
            for line in drift_lines(&fresh) {
                self.seams.print_line(line);
            }
            if !self.seams.confirm()? {
                return Err(Error::Plan(
                    "apply aborted: answer reads no 'yes'".to_string(),
                ));
            }
        }
        let notify_written;
        let notify = if let Some(sender) = self.seams.progress.clone() {
            notify_written = move |route: &Route| {
                let _ = sender.send(Event::DocumentWritten {
                    path: route.display(),
                });
            };
            Some(&notify_written as &dyn Fn(&Route))
        } else {
            None
        };
        let written =
            self.seams
                .applier
                .write_documents(&built.manifest.documents, &changed, notify)?;
        let removed = self
            .seams
            .applier
            .remove_orphans(&self.previous.manifest.documents, &built.manifest.documents)?;
        let removed = removed
            + self.seams.applier.remove_tree_members(
                &self.previous.manifest.documents,
                &built.manifest.documents,
            )?;
        self.seams
            .emit_writing_manifest(built.manifest.documents.len());
        let stored = self
            .seams
            .stores
            .slots()
            .store(&built, self.seams.progress.as_ref())?;
        self.seams.stores.blobs().prune()?;
        let fs: &dyn Filesystem = self.seams.fs;
        self.run_hooks(&built, &rt, fs, probe, &changed_ids)?;
        Ok(ApplyReport {
            written,
            removed,
            stored,
        })
    }

    /// Runs built hooks after files, state, and history land.
    ///
    /// Closed gates skip with a line, passing checks skip silently.
    /// Failures abort the rest.
    fn run_hooks(
        &mut self,
        built: &Bundle,
        rt: &Runtime,
        fs: &dyn Filesystem,
        probe: &dyn PathProbe,
        changed: &BTreeSet<String>,
    ) -> Result<()> {
        let total = built.manifest.hooks.len();
        let real = OsRunner;
        let runner: &dyn HookRunner = match self.seams.hook_runner {
            Some(runner) => runner,
            None => &real,
        };
        let ctx = HookCtx {
            rt,
            fs,
            probe,
            changed,
        };
        for (index, hook) in built.manifest.hooks.iter().enumerate() {
            let position = index + 1;
            if let Some(line) = gate_line(hook, &ctx) {
                self.seams.print_line(line);
                continue;
            }
            if !hook.checks.is_empty()
                && hook
                    .checks
                    .iter()
                    .all(|check| ctx.rt.evaluate(check, ctx.probe, ctx.changed))
            {
                let line = format!("skipped: {} (checks pass)", Arg::join(&hook.argv));
                self.seams.print_line(line);
                continue;
            }
            self.spawn_hook(hook, position, total, &ctx, runner)?;
        }
        Ok(())
    }

    /// Runs one open hook through the runner.
    ///
    /// # Errors
    ///
    /// Unresolvable binaries, nonzero codes, and unmet
    /// post-checks fail as plan errors.
    fn spawn_hook(
        &mut self,
        hook: &confit_core::hook::Hook,
        position: usize,
        total: usize,
        ctx: &HookCtx<'_>,
        runner: &dyn HookRunner,
    ) -> Result<()> {
        let argv_text = Arg::join(&hook.argv);
        let binary =
            resolve_hook(hook, ctx.rt, ctx.probe, &self.seams.applier).ok_or_else(|| {
                let head = hook.argv.first().map(Arg::display).unwrap_or_default();
                Error::Plan(format!("hook '{argv_text}' cannot resolve '{head}'"))
            })?;
        let mut spawn: Vec<String> = vec![binary.display().to_string()];
        for slot in hook.argv.iter().skip(1) {
            match slot {
                Arg::Text(text) => spawn.push(text.clone()),
                Arg::Route(route) => spawn.push(
                    self.seams
                        .applier
                        .resolve(route)
                        .to_string_lossy()
                        .into_owned(),
                ),
            }
        }
        let line = format!("hook {position} of {total}: {argv_text}");
        self.seams.print_line(line.clone());
        if let Some(sender) = self.seams.progress.as_ref() {
            let _ = sender.send(Event::HookRunning {
                position,
                total,
                argv: argv_text.clone(),
            });
        }
        let mut path_dirs: Vec<std::path::PathBuf> = Vec::with_capacity(hook.path.len());
        for slot in &hook.path {
            match slot {
                Arg::Text(text) => path_dirs.push(std::path::PathBuf::from(text)),
                Arg::Route(route) => path_dirs.push(self.seams.applier.resolve(route)),
            }
        }
        let outcome = runner.run(&spawn, &path_dirs, hook.timeout_secs)?;
        if let Some(log) = self.seams.log_file.clone() {
            append_hook_log(ctx.fs, &log, &line, &outcome.output)?;
        }
        if outcome.code != 0 {
            return Err(Error::Plan(format!(
                "hook '{argv_text}' failed with code {}",
                outcome.code
            )));
        }
        verify_post_checks(hook, ctx)?;
        log::debug!("hook {position} of {total} ran code={}", outcome.code);
        Ok(())
    }
}

/// Renders the skip line for the first closed gate, else none.
fn gate_line(hook: &confit_core::hook::Hook, ctx: &HookCtx<'_>) -> Option<String> {
    let argv_text = Arg::join(&hook.argv);
    if let Some(gate) = hook.requires.as_ref()
        && !ctx.rt.evaluate(gate, ctx.probe, ctx.changed)
    {
        return Some(format!(
            "warn: {argv_text} cannot run ({})",
            describe_condition(gate)
        ));
    }
    if let Some(gate) = hook.when.as_ref()
        && !ctx.rt.evaluate(gate, ctx.probe, ctx.changed)
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
fn verify_post_checks(hook: &confit_core::hook::Hook, ctx: &HookCtx<'_>) -> Result<()> {
    if hook.checks.is_empty() {
        return Ok(());
    }
    let argv_text = Arg::join(&hook.argv);
    let failed: Vec<String> = hook
        .checks
        .iter()
        .filter(|check| !ctx.rt.evaluate(check, ctx.probe, ctx.changed))
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

/// Prefixes slot errors with the calling command name.
///
/// Core slot resolution reads command-neutral. Each action
/// names itself once here instead of repeating dispatch.
fn prefix_command(command: &'static str) -> impl FnOnce(Error) -> Error {
    move |error| match error {
        Error::Plan(detail) => Error::Plan(format!("{command}: {detail}")),
        other => other,
    }
}

/// Computes the changed destination routes for one apply run.
///
/// First runs hold every desired route. Steady runs hold plan
/// changes plus drifted destinations.
fn changed_paths(
    built: &Bundle,
    previous: &Bundle,
    drifts: &[Drift],
    first_run: bool,
) -> BTreeSet<Route> {
    use confit_core::plan::DocumentStatus;

    if first_run {
        return built
            .manifest
            .documents
            .iter()
            .map(|document| document.destination.clone())
            .collect();
    }
    let mut out = BTreeSet::new();
    for document in &built.manifest.documents {
        if !matches!(document.status(previous), DocumentStatus::Unchanged) {
            out.insert(document.destination.clone());
        }
    }
    for document in &built.manifest.documents {
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
