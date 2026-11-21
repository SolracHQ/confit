//! Writes evaluated diffs to disk for apply and restore.

use std::collections::BTreeSet;
use std::path::PathBuf;

use confit_core::error::{Terminal, TerminalResult, contextual, error, kind::ErrorKind};
use confit_differ::diff::Diff;
use confit_events::{
    EventHandles,
    protocol::{Applied, Phase, TaskKind, TaskNote},
};
use confit_model::arg::Arg;
use confit_model::routes::Route;
use confit_runtime::{HookDecision, HookRunner, Writer};
use confit_store::Stores;

/// Result counts for one commit execution.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct WriteReport {
    /// The applied counts and the written rows.
    #[serde(flatten)]
    pub applied: Applied,
    /// The number of files the execution saved into the rescue state.
    pub saved: usize,
}

pub struct Applier {
    /// The diff the execution previews and writes.
    pub diff: Diff,
    /// The flag that skips the prompts.
    pub force: bool,
    /// The trusted roots and content caches.
    pub stores: Stores,
    /// The UI communication channel.
    pub handles: EventHandles,
    /// The folder that holds one log file per hook.
    pub hooks_dir: PathBuf,
}

impl Applier {
    /// Writes the diff after the preview and the prompts.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the destination write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when the destination removal fails.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] when the state write fails.
    /// - [`confit_core::error::kind::ErrorKind::RotateHistory`] when the history rotation fails.
    /// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] when the blob prune fails.
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a hook fails.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] when a hook spawn fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] when a hook log write fails.
    pub fn execute(self) -> TerminalResult<WriteReport> {
        if !self.handles.confirm(self.diff.clone(), self.force) {
            return Ok(WriteReport {
                applied: Applied::default(),
                saved: 0,
            });
        }
        self.handles.phase_started(Phase::Persisting);
        let saved = self.persist()?;
        self.handles.phase_started(Phase::ApplyingChanges);
        let had_previous = !self.stores.states().is_first_execution();
        let applied = self.commit()?;
        self.handles.write_results(applied.clone());
        self.run_hooks()?;
        self.handles
            .done(had_previous, saved > 0, !self.diff.drift.is_empty());
        Ok(WriteReport { applied, saved })
    }

    /// Persists the pool entries and the rescue copy.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] when the pool persist or the rescue save fails.
    fn persist(&self) -> TerminalResult<usize> {
        let built = &self.diff.wanted;
        self.stores
            .resources()
            .persist(&built.handles())
            .terminal(ErrorKind::StoreState)?;
        self.stores.states().save_rescue(&self.diff.effective)
    }

    /// Writes the effective rows, then rotates the pool.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteDestination`] when the destination write fails.
    /// - [`confit_core::error::kind::ErrorKind::RemoveDestination`] when the destination removal fails.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] when the state write fails.
    /// - [`confit_core::error::kind::ErrorKind::RotateHistory`] when the history rotation fails.
    /// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] when the blob prune fails.
    fn commit(&self) -> TerminalResult<Applied> {
        let built = &self.diff.wanted;
        let writer = Writer::new(self.stores.clone());
        let total = Writer::apply_total(&self.diff.effective);
        let reporter = self.handles.start_task(TaskKind::Write, Some(total));
        let applied = writer.apply(&self.diff.effective, &reporter)?;
        reporter.finish(TaskNote::Closed);
        self.stores.states().save_applied(built)?;
        self.stores
            .prune_unreferenced()
            .terminal(ErrorKind::PruneBlobs)?;
        Ok(applied)
    }

    /// Runs the built hooks after the files and the history land.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a hook fails.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] when a hook spawn fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] when a hook log write fails.
    fn run_hooks(&self) -> TerminalResult<()> {
        let runner = HookRunner::new(self.handles.clone());
        let routes: BTreeSet<Route> = self
            .diff
            .effective
            .iter()
            .map(|change| change.path.clone())
            .collect();
        let decided = runner.decide_all(&self.diff.wanted.hooks, &routes)?;
        let total = self.diff.wanted.hooks.len();
        for (index, hook) in self.diff.wanted.hooks.iter().enumerate() {
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
                    let spawned = runner.spawn(hook, binary, position, total, &self.hooks_dir)?;
                    log::info!("{line} output in {}", spawned.log.display());
                    runner.confirm(&spawned)?;
                    runner.verify(hook, &decided.checks, &routes, &spawned.argv)?;
                }
            }
        }
        Ok(())
    }
}
