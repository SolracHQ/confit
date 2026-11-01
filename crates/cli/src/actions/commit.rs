//! Commit run
//! Prepared manifests to disk writes shared by apply and restore.

use std::path::PathBuf;

use confit_core::error::{
    ContextualCause, Terminal, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_model::arg::Arg;
use confit_runtime::manifest::stored;
use confit_runtime::prepare;
use confit_runtime::{HookDecision, HookRunner, Prepared, Writer};
use confit_store::Stores;
use confit_ui::{ToUi, UiHandles};

use crate::presentation::difference;
use crate::presentation::summary;

/// Successful commit run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReport {
    /// Count of documents written to disk.
    pub written: usize,
    /// Count of deleted documents and dropped tree members removed from disk.
    pub removed: usize,
}

/// Commit run from prepared manifests to disk writes.
pub struct CommitRunner {
    /// Prepared manifests with differences and changed routes.
    pub prepared: Prepared,
    /// First prompt skip with difference re-prompt.
    pub force: bool,
    /// Write capabilities for the run.
    pub stores: Stores,
    /// Renderer channels for prints and prompts.
    pub handles: UiHandles,
    /// Hook output folder holding one file per hook.
    pub hooks_dir: PathBuf,
}

impl CommitRunner {
    /// Send one line to the renderer.
    fn print(&self, line: impl Into<String>) {
        let _ = self.handles.to_ui.send(ToUi::Print(line.into()));
    }

    /// Ask one yes question on the renderer.
    fn confirm(&self) -> TerminalResult<bool> {
        let reply = self
            .handles
            .ask("Apply these changes? Type 'yes' to continue: ".to_owned())
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::ReadReply)?;
        Ok(reply.trim() == "yes")
    }

    /// Write prepared manifests with preview, difference prompts, and rotation.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadReply`] for prompt failures.
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
        if !self.force && !self.confirm()? {
            self.print("no yes passed, bye");
            return Ok(WriteReport {
                written: 0,
                removed: 0,
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
            if !self.confirm()? {
                self.print("no yes passed, bye");
                return Ok(WriteReport {
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
    fn commit(&self) -> TerminalResult<WriteReport> {
        let built = &self.prepared.desired;
        let previous = &self.prepared.previous;
        let changed = &self.prepared.changed;
        let writer = Writer {
            stores: self.stores.clone(),
            to_ui: self.handles.to_ui.clone(),
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
        Ok(WriteReport { written, removed })
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
