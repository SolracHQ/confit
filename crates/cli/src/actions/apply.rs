//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::collections::BTreeSet;
use std::path::PathBuf;

use confit_core::document::ManifestDocument;
use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::{Error, Result};
use confit_core::fs::{Filesystem, snapshot_document, snapshot_tree};
use confit_core::hook::{describe_condition, lifecycle_lines, resolve_hook};
use confit_core::ids::DocPath;
use confit_core::plan::Bundle;
use confit_core::runtime::{Runtime, evaluate};
use confit_core::store::blobs::prune_blobs;
use confit_core::store::bundle::load_bundle_input;
use confit_core::store::slots::{
    archive_previous, default_state_path, load_state, resolve_slot, write_manifest,
};
use confit_core::store::{remove_orphans, remove_tree_members, write_documents};

use crate::cli::ApplyArgs;
use crate::hooks::{HookRunner, OsRunner, append_hook_log};
use crate::presentation::summary::Summary;

use crate::seams::{Seams, evaluate_shared, log_processed, timed};

use confit_core::progress::Event;

/// Outcome of one successful apply run.
///
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
/// use confit_cli::seams::Seams;
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::plan::Bundle;
/// use std::io::Cursor;
/// use std::path::Path;
///
/// let fs = MemoryFs::new();
/// let mut input = Cursor::new("yes\n");
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
///     seams: Seams::memory(&fs, &mut input),
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
    /// use confit_cli::seams::Seams;
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
    /// let seams = Seams::memory(&fs, &mut input);
    /// let runner = ApplyRunner::from_args(&args, seams);
    /// assert!(matches!(runner, Ok(_) | Err(_)));
    /// ```
    pub fn from_args(args: &ApplyArgs, seams: Seams<'a>) -> Result<Self> {
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("");
        if raw.starts_with('@') || raw.starts_with('%') {
            let (slot_plan, _) =
                resolve_slot(Some(raw), seams.fs).map_err(prefix_command("apply"))?;
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
    /// use confit_cli::seams::Seams;
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
    /// let seams = Seams::memory(&fs, &mut input);
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
        let snapshot = |document: &ManifestDocument| snapshot_document(document, fs);
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
        let changed = changed_paths(&built, &self.previous, &baseline, first_run);
        if self.preview {
            let lifecycle = lifecycle_lines(&built.manifest.hooks, &self.previous.manifest.hooks);
            let evaluated = built.hook_preview(&rt, fs, &changed)?;
            let report = Summary {
                built: &built,
                previous: &self.previous,
                drift: &baseline,
                first_run,
                hook_lines: lifecycle.as_slice(),
                hook_evaluated: evaluated.as_slice(),
            };
            let text = report.render();
            self.seams.print_line(text);
        }
        if !self.force && !self.seams.confirm()? {
            return Err(Error::Plan(
                "apply aborted: answer reads no 'yes'".to_string(),
            ));
        }
        let fresh = reference.drift(&snapshot, &snapshot_tree, order);
        if fresh != baseline {
            for line in Drift::lines(&fresh) {
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
            notify_written = move |path: &DocPath| {
                let _ = sender.send(Event::DocumentWritten {
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
            write_manifest(&built, Some(state), fs, self.seams.progress.as_ref())?;
        }
        self.seams.emit_writing_plan(built.manifest.documents.len());
        let stored = archive_previous(&built, fs, self.seams.progress.as_ref())?;
        prune_blobs(fs)?;
        self.run_hooks(&built, &rt, fs, &changed)?;
        Ok(ApplyReport {
            written: built.manifest.documents.len(),
            removed,
            stored,
        })
    }

    /// Runs built hooks after files, state, plus history land.
    ///
    /// Changed gates answer against the apply-start set.
    /// Failures abort the rest.
    fn run_hooks(
        &mut self,
        built: &Bundle,
        rt: &Runtime,
        fs: &dyn Filesystem,
        changed: &BTreeSet<DocPath>,
    ) -> Result<()> {
        let total = built.manifest.hooks.len();
        let real = OsRunner;
        let runner: &dyn HookRunner = match self.seams.hook_runner {
            Some(runner) => runner,
            None => &real,
        };
        let ctx = HookCtx {
            runner,
            rt,
            fs,
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
                    .all(|check| evaluate(check, ctx.rt, ctx.fs, ctx.changed))
            {
                let line = format!("skipped: {} (checks pass)", hook.argv.join(" "));
                self.seams.print_line(line);
                continue;
            }
            self.spawn_hook(hook, position, total, &ctx)?;
        }
        Ok(())
    }

    /// Runs one open hook through the runner.
    ///
    /// # Errors
    ///
    /// Unresolvable binaries plus nonzero codes plus unmet
    /// post-checks fail as plan errors.
    fn spawn_hook(
        &mut self,
        hook: &confit_core::hook::Hook,
        position: usize,
        total: usize,
        ctx: &HookCtx<'_>,
    ) -> Result<()> {
        let argv_text = hook.argv.join(" ");
        let binary = resolve_hook(hook, ctx.rt, ctx.fs).ok_or_else(|| {
            let head = hook.argv.first().cloned().unwrap_or_default();
            Error::Plan(format!("hook '{argv_text}' cannot resolve '{head}'"))
        })?;
        let mut spawn: Vec<String> = vec![binary.display().to_string()];
        spawn.extend(hook.argv.iter().skip(1).cloned());
        let line = format!("hook {position} of {total}: {argv_text}");
        self.seams.print_line(line.clone());
        if let Some(sender) = self.seams.progress.as_ref() {
            let _ = sender.send(Event::HookRunning {
                position,
                total,
                argv: argv_text.clone(),
            });
        }
        let path_dirs: Vec<std::path::PathBuf> =
            hook.path.iter().map(std::path::PathBuf::from).collect();
        let outcome = ctx.runner.run(&spawn, &path_dirs, hook.timeout_secs)?;
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

/// Shared hook-run context for one apply run.
struct HookCtx<'x> {
    /// Runs hook subprocesses.
    runner: &'x dyn HookRunner,
    /// Holds the runtime facts under reading.
    rt: &'x Runtime,
    /// Holds the backend under stating.
    fs: &'x dyn Filesystem,
    /// Holds the changed document ids under reading.
    changed: &'x BTreeSet<DocPath>,
}

/// Renders the skip line for the first closed gate, else none.
///
/// Closed requires warns inability, closed when skips un-need.
fn gate_line(hook: &confit_core::hook::Hook, ctx: &HookCtx<'_>) -> Option<String> {
    let argv_text = hook.argv.join(" ");
    if let Some(gate) = hook.requires.as_ref()
        && !evaluate(gate, ctx.rt, ctx.fs, ctx.changed)
    {
        return Some(format!(
            "warn: {argv_text} cannot run ({})",
            describe_condition(gate)
        ));
    }
    if let Some(gate) = hook.when.as_ref()
        && !evaluate(gate, ctx.rt, ctx.fs, ctx.changed)
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
    let argv_text = hook.argv.join(" ");
    let failed: Vec<String> = hook
        .checks
        .iter()
        .filter(|check| !evaluate(check, ctx.rt, ctx.fs, ctx.changed))
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

/// Computes the changed document ids for one apply run.
///
/// # Arguments
///
/// * `built` - the plan under applying.
/// * `previous` - the slot plan backing lifecycle marks.
/// * `drifts` - the drift entries backing the preview.
/// * `first_run` - true while the state slot reads absent.
///
/// # Returns
///
/// The changed document ids.
fn changed_paths(
    built: &Bundle,
    previous: &Bundle,
    drifts: &[Drift],
    first_run: bool,
) -> BTreeSet<DocPath> {
    use confit_core::plan::DocumentStatus;

    if first_run {
        return built
            .manifest
            .documents
            .iter()
            .map(|document| document.path.clone())
            .collect();
    }
    let mut out = BTreeSet::new();
    for document in &built.manifest.documents {
        if !matches!(document.status(previous), DocumentStatus::Unchanged) {
            out.insert(document.path.clone());
        }
    }
    for document in &built.manifest.documents {
        let prefix = format!("{}/", document.path.as_str());
        for drift in drifts {
            let path = drift.path().as_str();
            if path == document.path.as_str() || path.starts_with(&prefix) {
                out.insert(document.path.clone());
                break;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::document::{ManifestData, ManifestDocument, ManifestMember};
    use confit_core::plan::sha256_hex;

    fn text_doc(path: &str, content: &str) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Text {
                content: content.to_string(),
                mode: None,
            },
        )
    }

    fn with_hashes(documents: Vec<ManifestDocument>) -> Bundle {
        let mut docs = documents;
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = docs;
        previous
    }

    #[test]
    fn changed_paths_first_run_holds_every_built_path() {
        let built = match Bundle::build(vec![text_doc("a", "x"), text_doc("b", "y")], Vec::new()) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let changed = changed_paths(&built, &Bundle::empty(), &[], true);
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&DocPath::new("a")));
        assert!(changed.contains(&DocPath::new("b")));
    }

    #[test]
    fn changed_paths_later_run_marks_drift_plus_rewrite() {
        let previous = with_hashes(vec![
            text_doc("quiet", "same"),
            text_doc("drifted", "same"),
            text_doc("rewritten", "old"),
        ]);
        let built = match Bundle::build(
            vec![
                text_doc("quiet", "same"),
                text_doc("drifted", "same"),
                text_doc("rewritten", "new"),
            ],
            Vec::new(),
        ) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let drifts = vec![Drift::Hunk {
            path: DocPath::new("drifted"),
            hunks: "diff".to_string(),
        }];
        let changed = changed_paths(&built, &previous, &drifts, false);
        assert_eq!(changed.len(), 2);
        assert!(changed.contains(&DocPath::new("drifted")));
        assert!(changed.contains(&DocPath::new("rewritten")));
        assert!(!changed.contains(&DocPath::new("quiet")));
    }

    #[test]
    fn changed_paths_tree_member_drift_maps_to_parent() {
        fn tree_doc() -> ManifestDocument {
            ManifestDocument::new(
                DocPath::new("fonts"),
                ManifestData::Tree {
                    members: vec![ManifestMember {
                        relative: "member.ttf".into(),
                        blob: sha256_hex(&[1]),
                        mode: 0o644,
                    }],
                },
            )
        }
        let previous = with_hashes(vec![tree_doc()]);
        let built = match Bundle::build(vec![tree_doc()], Vec::new()) {
            Ok(out) => out,
            Err(error) => panic!("plan builds: {error}"),
        };
        let drifts = vec![Drift::Missing {
            path: DocPath::new("fonts/member.ttf"),
        }];
        let changed = changed_paths(&built, &previous, &drifts, false);
        assert_eq!(changed.len(), 1);
        assert!(changed.contains(&DocPath::new("fonts")));
    }
}
