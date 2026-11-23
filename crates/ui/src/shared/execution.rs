//! The accumulated execution state.

use std::{
    collections::BTreeMap,
    io::{self, BufRead as _, Write as _},
};

use confit_core::error::Error;
use confit_differ::{
    diff::Diff,
    view::{View, ViewKind},
};
use confit_events::protocol::{Applied, InitCreated, Phase, TaskKind, TaskNote, TaskState};

use crate::shared::{
    components::{style::strExt as _, tables::hints},
    stages::{
        closing::ClosingStage as _, failure::FailureStage as _, outcome::OutcomeStage as _,
        preview::PreviewStage as _,
    },
    state::{
        APPLYING_CHANGES, EVALUATION, HookLog, HookRun, JobProgress, OutcomeMap, PACKAGING,
        PERSISTING, error_text, executed, finished,
    },
};

/// Execution state for one render.
pub(crate) struct Execution {
    /// Previewed view.
    view: Option<View>,
    /// Write outcomes by expanded host path.
    outcomes: OutcomeMap,
    /// Finished hook runs.
    hooks: HookLog,
    /// Finished counts.
    counts: BTreeMap<TaskKind, JobProgress>,
}

impl Execution {
    /// Builds empty execution state.
    pub(crate) fn new() -> Self {
        Self {
            view: None,
            outcomes: OutcomeMap::new(),
            hooks: HookLog::new(),
            counts: BTreeMap::new(),
        }
    }

    /// Returns one phase title.
    pub(crate) fn phase(&self, phase: Phase) -> Vec<String> {
        let title = match phase {
            Phase::Evaluation => EVALUATION.title(),
            Phase::Packaging => PACKAGING.title(),
            Phase::Persisting => PERSISTING.title(),
            Phase::ApplyingChanges => APPLYING_CHANGES.title(),
        };
        vec![String::new(), title, String::new()]
    }

    /// Stores one task update.
    pub(crate) fn task(&mut self, kind: &TaskKind, state: &TaskState) -> Vec<String> {
        match state {
            TaskState::Started { total } => {
                self.counts.insert(
                    kind.clone(),
                    JobProgress {
                        done: 0,
                        total: *total,
                    },
                );
                Vec::new()
            }
            TaskState::Progress { done, total, .. } => {
                self.counts.insert(
                    kind.clone(),
                    JobProgress {
                        done: *done,
                        total: *total,
                    },
                );
                Vec::new()
            }
            TaskState::Finished { note } => {
                if let (TaskKind::Hook { argv, .. }, TaskNote::Spawned(spawned)) = (kind, note) {
                    self.hooks.push(HookRun {
                        argv: argv.clone(),
                        spawned: spawned.clone(),
                    });
                    vec![executed(spawned)]
                } else {
                    let job = self.counts.get(kind).copied().unwrap_or_default();
                    if let Some(line) = finished(kind, job.done, job.total, note) {
                        vec![line]
                    } else {
                        Vec::new()
                    }
                }
            }
        }
    }

    /// Stores the preview.
    pub(crate) fn preview(&mut self, diff: &Diff, view: ViewKind) -> Vec<String> {
        let picked = diff.view(view);
        self.view = Some(picked.clone());
        let mut out = vec![String::new()];
        out.extend(picked.preview());
        out
    }

    /// Returns the stored preview view.
    pub(crate) fn view(&self) -> Option<&View> {
        self.view.as_ref()
    }

    /// Stores the write results.
    pub(crate) fn write_results(&mut self, applied: &Applied) -> Vec<String> {
        self.outcomes = OutcomeMap::from_applied(applied);
        self.view
            .as_ref()
            .map(|view| view.outcome(&self.outcomes))
            .unwrap_or_default()
    }

    /// Builds the created tree with its count line.
    pub(crate) fn init_results(&self, created: &InitCreated) -> Vec<String> {
        crate::shared::stages::init::init_results(&created.root, &created.files)
    }

    /// Builds the closing table and restore hints.
    pub(crate) fn close(&self, undo: bool, rescued: bool, drifted: bool) -> Vec<String> {
        match self.view.as_ref() {
            Some(view) => view.close(&self.outcomes, undo, rescued, drifted),
            None => hints(undo, rescued, drifted),
        }
    }

    /// Returns the failure report.
    pub(crate) fn failure(&self, error: &Error) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(view) = &self.view {
            out.extend(view.failure(&self.outcomes, &self.hooks));
        }
        out.extend(error_text(error));
        out
    }
}

/// Reads one input line.
///
/// # Errors
///
/// - [`std::io::Error`] when stderr or stdin fails.
pub(crate) fn read_answer(question: &str) -> io::Result<String> {
    let mut stderr = io::stderr().lock();
    stderr.write_all(question.as_bytes())?;
    stderr.flush()?;
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}
