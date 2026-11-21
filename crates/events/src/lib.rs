//! Worker channels with prompt and confirm helpers.

pub mod protocol;

use confit_core::error::Error;
use confit_differ::diff::Diff;
use crossbeam_channel::Receiver;
use crossbeam_channel::Sender;

use crate::protocol::{Applied, FromUi, Phase, RowUpdate, TaskKind, TaskNote, TaskState, ToUi};

/// Panics when the render thread dies.
const UI_THREAD_DIED: &str = "UI thread died unexpectedly, this must never happen. If you see this message please create an issue with the instructions to replicate it on github.";
/// Panics when the main thread dies.
const MAIN_THREAD_DIED: &str = "Main thread died unexpectedly, this must never happen. If you see this message please create an issue with the instructions to replicate it on github.";

/// The ui message channel pair.
#[derive(Debug, Clone)]
pub struct EventHandles {
    /// The outbound ToUi message sender.
    to_ui: Sender<ToUi>,
    /// The inbound FromUi message receiver.
    from_ui: Receiver<FromUi>,
}

impl EventHandles {
    /// Builds handles over the worker channels.
    pub fn new(to_ui: Sender<ToUi>, from_ui: Receiver<FromUi>) -> Self {
        Self { to_ui, from_ui }
    }

    /// Sends one event to the render thread.
    ///
    /// - A closed channel panics.
    pub fn send(&self, event: ToUi) {
        if self.to_ui.send(event).is_err() {
            panic!("{UI_THREAD_DIED}");
        }
    }

    /// Sends one event and returns whether the channel accepts it.
    pub fn try_send(&self, event: ToUi) -> bool {
        self.to_ui.send(event).is_ok()
    }

    /// Returns one answer from the render thread.
    ///
    /// - A closed channel panics.
    pub fn recv(&self) -> FromUi {
        match self.from_ui.recv() {
            Ok(reply) => reply,
            Err(_) => {
                panic!("{UI_THREAD_DIED}");
            }
        }
    }

    /// Sends one question to the render thread and returns
    /// its answer.
    ///
    /// - A closed channel panics.
    pub fn ask(&self, prompt: String) -> Option<String> {
        self.send(ToUi::Prompt(prompt));
        match self.recv() {
            FromUi::Answer(answer) => Some(answer),
            FromUi::Close => None,
            FromUi::Approved => None,
        }
    }

    /// Sends one approval and returns whether the execution continues.
    ///
    /// - A closed channel panics.
    pub fn confirm(&self, diff: Diff, force: bool) -> bool {
        self.send(ToUi::ShowAndConfirm { diff, force });
        match self.recv() {
            FromUi::Approved => true,
            FromUi::Close => false,
            FromUi::Answer(_) => false,
        }
    }

    /// Sends one phase start to the render thread.
    ///
    /// - A closed channel panics.
    pub fn phase_started(&self, phase: Phase) {
        self.send(ToUi::PhaseStarted(phase));
    }

    /// Sends one check preview to the render thread.
    ///
    /// - A closed channel panics.
    pub fn desired_preview(&self, diff: Diff) {
        self.send(ToUi::DesiredPreview(diff));
    }

    /// Sends the write results to the render thread.
    ///
    /// - A closed channel panics.
    pub fn write_results(&self, applied: Applied) {
        self.send(ToUi::WriteResults(applied));
    }

    /// Sends one failure notice to the render thread.
    ///
    /// - A closed channel panics.
    pub fn error(&self, error: Box<Error>) {
        self.send(ToUi::Error(error));
    }

    /// Sends one failure notice and returns whether the channel accepts it.
    pub fn try_error(&self, error: Box<Error>) -> bool {
        self.try_send(ToUi::Error(error))
    }

    /// Opens one background job and returns its reporter.
    ///
    /// - A closed channel panics.
    pub fn start_task(&self, kind: TaskKind, total: Option<usize>) -> TaskReporter {
        self.send(ToUi::Task {
            kind: kind.clone(),
            state: TaskState::Started { total },
        });
        TaskReporter {
            handles: self.clone(),
            kind,
        }
    }

    /// Sends one close notice to the render thread.
    ///
    /// - A closed channel panics.
    pub fn done(&self, undo: bool, rescued: bool, drifted: bool) {
        self.send(ToUi::Done {
            undo,
            rescued,
            drifted,
        });
    }

    /// Builds memory channels with no render thread.
    ///
    /// - Other questions panic with a closed channel.
    pub fn silent() -> Self {
        let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (replies, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        std::thread::spawn(move || {
            for event in incoming {
                if matches!(event, ToUi::ShowAndConfirm { force: true, .. }) {
                    if replies.send(FromUi::Approved).is_err() {
                        break;
                    }
                } else if matches!(event, ToUi::Prompt(_) | ToUi::ShowAndConfirm { .. }) {
                    break;
                }
            }
        });
        Self { to_ui, from_ui }
    }
}

/// One background job reporter over the worker channels.
#[derive(Debug, Clone)]
pub struct TaskReporter {
    /// The worker channel pair.
    handles: EventHandles,
    /// The job behind the updates.
    kind: TaskKind,
}

impl TaskReporter {
    /// Sends one progress update for the job.
    ///
    /// - A closed channel panics.
    pub fn progress(&self, done: usize, total: Option<usize>, row: Option<RowUpdate>) {
        self.handles.send(ToUi::Task {
            kind: self.kind.clone(),
            state: TaskState::Progress { done, total, row },
        });
    }

    /// Sends one closing fact for the job.
    ///
    /// - A closed channel panics.
    pub fn finish(&self, note: TaskNote) {
        self.handles.send(ToUi::Task {
            kind: self.kind.clone(),
            state: TaskState::Finished { note },
        });
    }
}

/// Builds both ends with the channels hidden inside.
pub fn pair() -> (EventHandles, UiHandles) {
    let (to_ui, incoming) = crossbeam_channel::unbounded();
    let (replies, from_ui) = crossbeam_channel::unbounded();
    (
        EventHandles::new(to_ui, from_ui),
        UiHandles::new(incoming, replies),
    )
}

/// The render-side channel pair.
#[derive(Debug, Clone)]
pub struct UiHandles {
    /// The inbound ToUi message receiver.
    incoming: Receiver<ToUi>,
    /// The outbound FromUi message sender.
    replies: Sender<FromUi>,
}

impl UiHandles {
    /// Builds render handles over the worker channels.
    pub fn new(incoming: Receiver<ToUi>, replies: Sender<FromUi>) -> Self {
        Self { incoming, replies }
    }

    /// Returns the next worker event.
    pub fn recv(&self) -> Option<ToUi> {
        self.incoming.recv().ok()
    }

    /// Returns the next worker event without waiting.
    pub fn try_recv(&self) -> Option<ToUi> {
        self.incoming.try_recv().ok()
    }

    /// Sends one reply to the worker.
    ///
    /// - A lost main thread panics.
    pub fn send(&self, reply: FromUi) {
        if self.replies.send(reply).is_err() {
            panic!("{MAIN_THREAD_DIED}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::sha::Sha;
    use confit_differ::{
        change::{Change, ChangeKind, Comparability, Side},
        diff::Diff,
        view::{View, ViewKind},
    };
    use confit_model::document::FileMode;
    use confit_model::manifest::Manifest;
    use confit_model::routes::{Route, RouteBase};
    use std::sync::Arc;

    /// Builds one empty manifest for a diff side.
    fn empty_manifest() -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents: Vec::new(),
            hooks: Vec::new(),
        }
    }

    /// Builds one addition change for a literal route.
    fn addition(route: Route, sha: Sha) -> Change {
        let expanded = route.resolve().unwrap();
        Change {
            path: route,
            expanded,
            kind: ChangeKind::Addition,
            comparability: Comparability::Comparable,
            group: None,
            before: None,
            after: Some(Side::new(sha, FileMode::default())),
        }
    }

    /// Builds one empty diff with no rows.
    fn empty_diff() -> Diff {
        let wanted = Arc::new(empty_manifest());
        let recorded = Arc::new(empty_manifest());
        Diff {
            wanted: Arc::clone(&wanted),
            recorded: Arc::clone(&recorded),
            desired: View::new(
                ViewKind::Desired,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
            drift: View::new(
                ViewKind::Drift,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
            effective: View::new(
                ViewKind::Effective,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
        }
    }

    #[test]
    fn confirm_returns_true_on_approval() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        let route = Route::new(RouteBase::Literal, ".zshrc").unwrap();
        let wanted = Arc::new(empty_manifest());
        let recorded = Arc::new(empty_manifest());
        let expected = Diff {
            wanted: Arc::clone(&wanted),
            recorded: Arc::clone(&recorded),
            desired: View::new(
                ViewKind::Desired,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
            drift: View::new(
                ViewKind::Drift,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
            effective: View::new(
                ViewKind::Effective,
                vec![addition(route, Sha::hash(b"hi"))],
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
        };
        outgoing.send(FromUi::Approved).unwrap();
        assert!(handles.confirm(expected, false));
    }

    #[test]
    fn confirm_returns_false_on_close() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        outgoing.send(FromUi::Close).unwrap();
        assert!(!handles.confirm(empty_diff(), false));
    }

    #[test]
    fn silent_approves_force_without_a_reader() {
        let handles = EventHandles::silent();
        assert!(handles.confirm(empty_diff(), true));
    }

    #[test]
    fn ask_returns_the_answer_line() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        outgoing.send(FromUi::Answer("hello".to_owned())).unwrap();
        let found = handles.ask("proceed?".to_owned());
        assert_eq!(found, Some("hello".to_owned()));
    }

    #[test]
    fn ask_returns_none_on_close() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        outgoing.send(FromUi::Close).unwrap();
        assert_eq!(handles.ask("proceed?".to_owned()), None);
    }
}
