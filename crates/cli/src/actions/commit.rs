//! Writes prepared manifests to disk for apply and restore.

use std::path::PathBuf;

use confit_core::error::{Terminal, TerminalResult, contextual, error, kind::ErrorKind};
use confit_events::{EventHandles, ToUi};
use confit_model::arg::Arg;
use confit_runtime::prepare;
use confit_runtime::snapshot;
use confit_runtime::{HookDecision, HookRunner, Prepared, Writer};
use confit_store::Stores;

use crate::presentation::difference;
use crate::presentation::summary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReport {
    /// The number of documents the run wrote to disk.
    pub written: usize,
    /// The number of documents and tree members the run removed.
    pub removed: usize,
    /// The number of files the run saved into the rescue state.
    pub saved: usize,
}

pub struct CommitRunner {
    /// The manifests to write, with their differences and changed routes.
    pub prepared: Prepared,
    /// The flag that skips the prompts.
    pub force: bool,
    /// The stores that hold the project and the state.
    pub stores: Stores,
    /// The channels that carry the prints and the prompts.
    pub handles: EventHandles,
    /// The folder that holds one log file per hook.
    pub hooks_dir: PathBuf,
}

impl CommitRunner {
    /// Sends one line to the renderer and ignores a closed channel.
    fn print(&self, line: impl Into<String>) {
        let _ = self.handles.to_ui.send(ToUi::Print(line.into()));
    }

    /// Writes the prepared manifests after the preview and the prompts.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] when the state refresh fails.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the state refresh evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when the state refresh Lua run fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the destination write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when the destination removal fails.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the removal evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] when the state write fails.
    /// - [`confit_core::error::kind::ErrorKind::ApplyState`] when the state refresh fails.
    /// - [`confit_core::error::kind::ErrorKind::RotateHistory`] when the history rotation fails.
    /// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] when the blob prune fails.
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a hook run fails.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] when a hook spawn fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] when a hook log write fails.
    pub fn execute(self) -> TerminalResult<WriteReport> {
        self.print(summary::render_prepared(&self.prepared));
        for warning in &self.prepared.warnings {
            self.print(warning.summary());
            log::warn!(
                "diff warning: {} context: {}",
                warning.summary(),
                warning.context()
            );
        }
        if !self.force && self.handles.confirm().is_err() {
            self.print("no confirmation, the run stops");
            return Ok(WriteReport {
                written: 0,
                removed: 0,
                saved: 0,
            });
        }
        let (fresh, fresh_warnings) = prepare::refresh(&self.stores, &self.prepared)?;
        for warning in &fresh_warnings {
            self.print(warning.summary());
            log::warn!(
                "diff warning: {} context: {}",
                warning.summary(),
                warning.context()
            );
        }
        if fresh != self.prepared.differences {
            for line in difference::difference_lines(&fresh) {
                self.print(line);
            }
        }
        let report = self.commit()?;
        self.run_hooks()?;
        Ok(report)
    }

    /// Writes the built documents, removes deleted ones, and rotates the pool.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the destination write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when the destination removal fails.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the removal evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] when the state write fails.
    /// - [`confit_core::error::kind::ErrorKind::RotateHistory`] when the history rotation fails.
    /// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] when the blob prune fails.
    fn commit(&self) -> TerminalResult<WriteReport> {
        let built = &self.prepared.desired;
        let previous = &self.prepared.previous;
        let changed = &self.prepared.changed;
        self.stores
            .resources()
            .persist(&built.handles())
            .terminal(ErrorKind::StoreState)?;
        let capture = snapshot::capture(&self.stores, &self.prepared)?;
        let mut saved = 0;
        if capture.files > 0 {
            self.stores
                .states()
                .save_named(snapshot::RESCUE_NAME, &capture.manifest)?;
            saved = capture.files;
        }
        for skipped in &capture.skipped {
            self.print(format!("skipped: {skipped}"));
        }
        let writer = Writer {
            stores: self.stores.clone(),
            to_ui: self.handles.to_ui.clone(),
        };
        let total = Writer::apply_total(&built.documents, &previous.documents);
        let _ = self.handles.to_ui.send(ToUi::ApplyStarted { total });
        let written = writer.write_documents(&built.documents, changed)?;
        let removed = writer.remove_deleted(&previous.documents, &built.documents)?;
        let removed =
            removed + writer.remove_tree_members(&previous.documents, &built.documents)?;
        let _ = self.handles.to_ui.send(ToUi::ApplyFinished);
        self.stores.states().save_applied(built)?;
        self.stores
            .prune_unreferenced()
            .terminal(ErrorKind::PruneBlobs)?;
        Ok(WriteReport {
            written,
            removed,
            saved,
        })
    }

    /// Runs the built hooks after the files and the history land.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a hook run fails.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] when a hook spawn fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] when a hook log write fails.
    fn run_hooks(&self) -> TerminalResult<()> {
        let runner = HookRunner {
            to_ui: self.handles.to_ui.clone(),
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
                    self.print(line.clone());
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
