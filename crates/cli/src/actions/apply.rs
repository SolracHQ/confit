//! Apply run
//! Desired documents to disk writes with prompts.

use std::io::BufRead;
use std::io::Write as _;
use std::path::PathBuf;

use confit_core::error::{Error, ErrorCause, ErrorContext, Failure, HookReason, Mixed, Outcome};
use confit_core::sha::Sha;
use confit_engine::Engine;
use confit_model::arg::Arg;
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::stored;
use confit_runtime::{HookDecision, HookRunner, Prepared, Writer, prepare, refresh};
use confit_store::Stores;
use confit_store::state::Picker;

use crate::cli::{ApplyArgs, plan_profile};
use crate::presentation::difference::difference_lines;
use crate::presentation::summary::render_prepared;

use crate::sinks::Sinks;

/// Successful apply run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Count of documents written to disk.
    pub written: usize,
    /// Count of deleted documents and dropped tree members removed from disk.
    pub removed: usize,
}

/// Apply run from desired documents to disk writes.
pub struct ApplyRunner<'a> {
    /// Prepared manifests with differences and changed routes.
    pub prepared: Prepared,
    /// First prompt skip with difference re-prompt.
    pub force: bool,
    /// Stdin confirmation reader.
    pub input: &'a mut dyn BufRead,
    /// Write capabilities for the run.
    pub stores: Stores,
    /// Output senders for the run.
    pub sinks: Sinks,
    /// Hook output folder holding one file per hook.
    pub hooks_dir: PathBuf,
    /// Source text holding the slot context for state mapping.
    pub source: String,
}

impl<'a> ApplyRunner<'a> {
    /// Resolve desired documents from flags with explicit run fields.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ApplySlot`] for malformed slots.
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] for unreadable states and bundles.
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] for unreadable states and bundles.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] for unreadable states and bundles.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable states and bundles.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unreadable states and bundles.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for unreadable states and bundles.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::MissingProfile`] for absent profiles.
    /// - [`confit_core::error::kind::ErrorKind::MissingProject`] for absent projects.
    /// - [`confit_core::error::kind::ErrorKind::FlagsConflict`] for conflicting flags.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] for stored states.
    pub fn from_args(
        args: &ApplyArgs,
        input: &'a mut dyn BufRead,
        stores: Stores,
        sinks: Sinks,
        hooks_dir: PathBuf,
    ) -> Outcome<Self> {
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("").to_owned();
        let bundles = Bundler::new(&stores);
        let desired = if raw.starts_with('@') || raw.starts_with('%') {
            let picker = Self::map_state(&raw, raw.parse::<Picker>().map_err(Failure::Mid))?;
            Self::map_state(&raw, stored::resolve(&stores.states(), &picker))?
        } else if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            bundles.read(positional)?
        } else if positional.is_dir() {
            let path = plan_profile(positional, args.profile.as_deref());
            if args.profile.is_none() && !path.exists() {
                return Err(Error::missing_project(positional.to_path_buf()));
            }
            let evaluated = Engine::new(stores.clone()).evaluate(&path)?;
            let built = prepare::build(evaluated.documents, evaluated.hooks)?;
            built.persist(&stores).map_err(Error::store_state)?;
            built
        } else {
            if args.profile.is_some() {
                return Err(Error::flags_conflict(
                    "apply".to_owned(),
                    vec!["--profile".to_owned(), positional.display().to_string()],
                ));
            }
            if !positional.exists() {
                return Err(Error::missing_profile(positional.to_path_buf()));
            }
            let evaluated = Engine::new(stores.clone()).evaluate(positional)?;
            let built = prepare::build(evaluated.documents, evaluated.hooks)?;
            built.persist(&stores).map_err(Error::store_state)?;
            built
        };
        let prepared = Self::map_state(&raw, prepare(&stores, desired))?;
        Ok(Self {
            prepared,
            force: args.force,
            input,
            stores,
            sinks,
            hooks_dir,
            source: raw,
        })
    }

    /// Map one mixed state result to its apply terminal.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ApplySlot`] for malformed slots and state faults.
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] for malformed slots and state faults.
    fn map_state<T>(raw: &str, result: Mixed<T>) -> Outcome<T> {
        match result {
            Ok(found) => Ok(found),
            Err(Failure::Terminal(terminal)) => Err(terminal),
            Err(Failure::Mid(error)) => match error.root_cause() {
                ErrorCause::SlotReason(reason) => {
                    Err(Error::apply_slot(raw.to_owned(), reason.clone()))
                }
                _ => Err(Error::apply_state(error)),
            },
        }
    }

    /// Apply desired documents with preview, difference prompts, and rotation.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadReply`] for prompt failures.
    /// - [`confit_core::error::kind::ErrorKind::ApplySlot`] for state refresh faults.
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] for state refresh faults.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for state refresh faults.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for state refresh faults.
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] for destination writes and removals.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] for destination writes and removals.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for destination writes and removals.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::RotateHistory`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for hook runs.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] for hook runs.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] for hook runs.
    pub fn execute(self) -> Outcome<ApplyReport> {
        self.sinks.print_line(render_prepared(&self.prepared));
        for warning in &self.prepared.warnings {
            self.sinks.print_line(warning.summary());
            log::warn!(
                "diff warning: {} chain: {:?}",
                warning.summary(),
                warning.chain()
            );
        }
        if !self.force && !self.sinks.confirm(self.input)? {
            self.sinks.print_line("no yes passed, bye".to_owned());
            return Ok(ApplyReport {
                written: 0,
                removed: 0,
            });
        }
        let (fresh, fresh_warnings) =
            Self::map_state(&self.source, refresh(&self.stores, &self.prepared))?;
        for warning in &fresh_warnings {
            self.sinks.print_line(warning.summary());
            log::warn!(
                "diff warning: {} chain: {:?}",
                warning.summary(),
                warning.chain()
            );
        }
        if fresh != self.prepared.differences {
            for line in difference_lines(&fresh) {
                self.sinks.print_line(line);
            }
            if !self.sinks.confirm(self.input)? {
                self.sinks.print_line("no yes passed, bye".to_owned());
                return Ok(ApplyReport {
                    written: 0,
                    removed: 0,
                });
            }
        }
        let report = self.commit()?;
        self.run_hooks()?;
        Ok(report)
    }

    /// Write built documents with deleted document removal and pool rotation.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] for destination writes and removals.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] for destination writes and removals.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for destination writes and removals.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::RotateHistory`] for stored states.
    /// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] for pruned blobs.
    fn commit(&self) -> Outcome<ApplyReport> {
        let built = &self.prepared.desired;
        let previous = &self.prepared.previous;
        let changed = &self.prepared.changed;
        let writer = Writer {
            stores: self.stores.clone(),
            progress: self.sinks.progress.clone(),
        };
        let written = writer.write_documents(&built.documents, changed)?;
        let removed = writer.remove_deleted(&previous.documents, &built.documents)?;
        let removed =
            removed + writer.remove_tree_members(&previous.documents, &built.documents)?;
        stored::store(&self.stores.states(), built)?;
        let keep = stored::keep_set(&self.stores.states()).map_err(Error::apply_state)?;
        self.stores
            .pool()
            .prune(&keep)
            .map_err(Error::prune_blobs)?;
        Ok(ApplyReport { written, removed })
    }

    /// Run built hooks after files, state, and history land.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for hook runs.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] for hook runs.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] for hook runs.
    fn run_hooks(&self) -> Outcome<()> {
        let runner = HookRunner {
            progress: self.sinks.progress.clone(),
        };
        let prepared = &self.prepared;
        let decided = runner.decide_all(&prepared.desired.hooks, &prepared.changed)?;
        let total = prepared.desired.hooks.len();
        for (index, hook) in prepared.desired.hooks.iter().enumerate() {
            let position = index + 1;
            match &decided.decisions[index] {
                HookDecision::RequiresClosed(blocked) => {
                    return Err(Error::hook_failed(
                        Arg::join(&hook.argv),
                        HookReason::of(blocked),
                        ErrorContext::from(blocked.clone()),
                    ));
                }
                HookDecision::WhenClosed(condition, _) => {
                    log::info!(
                        "hook {position} of {total} skips, when-guard closed for {}: {}",
                        Arg::join(&hook.argv),
                        condition.describe()
                    );
                }
                HookDecision::ChecksPass => {
                    log::info!(
                        "hook {position} of {total} skips, checks pass for {}",
                        Arg::join(&hook.argv)
                    );
                }
                HookDecision::Run(binary) => {
                    let line = format!("hook {position} of {total}: {}", Arg::join(&hook.argv));
                    self.sinks.print_line(line.clone());
                    let spawned = runner.spawn(hook, binary, position, total)?;
                    let file_name = hook_file_name(&spawned.argv);
                    write_hook_log(&self.hooks_dir, &file_name, &line, &spawned.output)?;
                    log::info!(
                        "{line} output in {}",
                        self.hooks_dir.join(&file_name).display()
                    );
                    runner.confirm(&spawned)?;
                    runner.verify(hook, &decided.checks, &prepared.changed, &spawned.argv)?;
                }
            }
        }
        Ok(())
    }
}

/// Build one hook log file name from its slug and hash.
fn hook_file_name(argv: &str) -> String {
    let mut slug = argv
        .split(|cell: char| !cell.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        slug = "hook".to_owned();
    }
    if slug.len() > 40 {
        slug.truncate(40);
    }
    let hash = Sha::hash(argv.as_bytes()).short();
    format!("{slug}-{hash}.log")
}

/// Write one hook log file with parent folders created as needed.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] for failed log writes.
fn write_hook_log(
    hooks_dir: &std::path::Path,
    file_name: &str,
    header: &str,
    output: &[u8],
) -> Outcome<()> {
    use confit_core::error::Terminal as _;

    let log = hooks_dir.join(file_name);
    std::fs::create_dir_all(hooks_dir).write_hook_log(&log)?;
    let mut sink = std::fs::File::create(&log).write_hook_log(&log)?;
    sink.write_all(header.as_bytes()).write_hook_log(&log)?;
    sink.write_all(b"\n").write_hook_log(&log)?;
    sink.write_all(output).write_hook_log(&log)?;
    if !output.ends_with(b"\n") {
        sink.write_all(b"\n").write_hook_log(&log)?;
    }
    sink.sync_all().write_hook_log(&log)?;
    Ok(())
}
