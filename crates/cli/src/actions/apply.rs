//! Apply run
//! Desired documents to disk writes with prompts.

use std::io::BufRead;
use std::path::PathBuf;

use confit_core::error::{
    ErrorCause, FilesystemError, Terminal, TerminalResult, UsageError, WithContext, contextual,
    error, kind::ErrorKind,
};
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
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::ReadBundle`] for unreadable bundles.
    /// - [`confit_core::error::kind::ErrorKind::UnpackMember`] for unreadable bundles.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for unreadable bundles.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated documents.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable projects.
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
    ) -> TerminalResult<Self> {
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("").to_owned();
        let bundles = Bundler::new(&stores);
        let desired = if raw.starts_with('@') || raw.starts_with('%') {
            let picker = match raw.parse::<Picker>().with_slot(&raw) {
                Ok(picker) => picker,
                Err(contextual) => {
                    let kind = match contextual.cause() {
                        ErrorCause::SlotReason(_) => ErrorKind::ApplySlot,
                        _ => ErrorKind::ApplyState,
                    };
                    return error(kind, contextual);
                }
            };
            stored::resolve(&stores.states(), &picker)?
        } else if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            bundles.read(positional)?
        } else if positional.is_dir() {
            let path = plan_profile(positional, args.profile.as_deref());
            if args.profile.is_none() && !path.exists() {
                return error(
                    ErrorKind::MissingProject,
                    contextual(FilesystemError::FileMissing).with_project(positional),
                );
            }
            let evaluated = Engine::new(stores.clone()).evaluate(&path)?;
            let built = prepare::build(evaluated.documents, evaluated.hooks)?;
            built.persist(&stores).terminal(ErrorKind::StoreState)?;
            built
        } else {
            if args.profile.is_some() {
                return error(
                    ErrorKind::FlagsConflict,
                    contextual(UsageError::FlagsConflict)
                        .with_command("apply")
                        .with_flags(vec!["--profile".to_owned(), raw.clone()]),
                );
            }
            if !positional.exists() {
                return error(
                    ErrorKind::MissingProfile,
                    contextual(FilesystemError::FileMissing).with_profile(positional),
                );
            }
            let evaluated = Engine::new(stores.clone()).evaluate(positional)?;
            let built = prepare::build(evaluated.documents, evaluated.hooks)?;
            built.persist(&stores).terminal(ErrorKind::StoreState)?;
            built
        };
        let prepared = prepare(&stores, desired)?;
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

    /// Map one slot parse to its apply terminal.
    ///
    /// # Errors
    ///
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
    pub fn execute(self) -> TerminalResult<ApplyReport> {
        self.sinks.print_line(render_prepared(&self.prepared));
        for warning in &self.prepared.warnings {
            self.sinks.print_line(warning.summary());
            log::warn!(
                "diff warning: {} context: {}",
                warning.summary(),
                warning.context()
            );
        }
        if !self.force && !self.sinks.confirm(self.input)? {
            self.sinks.print_line("no yes passed, bye".to_owned());
            return Ok(ApplyReport {
                written: 0,
                removed: 0,
            });
        }
        let (fresh, fresh_warnings) = refresh(&self.stores, &self.prepared)?;
        for warning in &fresh_warnings {
            self.sinks.print_line(warning.summary());
            log::warn!(
                "diff warning: {} context: {}",
                warning.summary(),
                warning.context()
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
    fn commit(&self) -> TerminalResult<ApplyReport> {
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
        let keep = stored::keep_set(&self.stores.states()).terminal(ErrorKind::ApplyState)?;
        self.stores
            .pool()
            .prune(&keep)
            .terminal(ErrorKind::PruneBlobs)?;
        Ok(ApplyReport { written, removed })
    }

    /// Run built hooks after files, state, and history land.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for hook runs.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] for hook runs.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] for hook runs.
    fn run_hooks(&self) -> TerminalResult<()> {
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
                    return error(
                        ErrorKind::RunHook,
                        contextual(blocked.clone()).with_argv(Arg::join(&hook.argv)),
                    );
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
                    let spawned = runner.spawn(hook, binary, position, total, &self.hooks_dir)?;
                    log::info!("{line} output in {}", spawned.log.display());
                    runner.confirm(&spawned)?;
                    runner.verify(hook, &decided.checks, &prepared.changed, &spawned.argv)?;
                }
            }
        }
        Ok(())
    }
}
