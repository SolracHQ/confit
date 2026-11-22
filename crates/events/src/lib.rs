//! Worker channels with prompt and confirm helpers.

pub mod protocol;

use confit_core::error::Error;
use confit_differ::{diff::Diff, view::ViewKind};
use crossbeam_channel::{Receiver, Sender};

use crate::protocol::{
    Applied, FromUi, Phase, PromptKind, RowUpdate, TaskKind, TaskNote, TaskState, ToUi,
};

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

    /// Sends an event to the render thread.
    ///
    /// - A closed channel panics.
    pub fn send(&self, event: ToUi) {
        if self.to_ui.send(event).is_err() {
            panic!("{UI_THREAD_DIED}");
        }
    }

    /// Returns true if the channel accepts the event.
    pub fn try_send(&self, event: ToUi) -> bool {
        self.to_ui.send(event).is_ok()
    }

    /// Returns the answer from the render thread.
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

    /// Sends a question ask and waits for the user reply.
    pub fn ask(&self, prompt: String) -> Option<String> {
        self.send(ToUi::Prompt(prompt));
        match self.recv() {
            FromUi::Answer(answer) => Some(answer),
            FromUi::Close => None,
            FromUi::Approved => None,
        }
    }

    /// Sends a preview to the render thread.
    ///
    /// - A closed channel panics.
    pub fn preview(&self, diff: Diff, view: ViewKind) {
        self.send(ToUi::Preview { diff, view });
    }

    /// Sends a details ask and waits for the user reply.
    pub fn ask_for_details(&self) {
        self.send(ToUi::AskForDetails);
        let _ = self.recv();
    }

    /// Sends a confirm ask and waits for the user reply.
    pub fn ask_for_confirm(&self, prompt: PromptKind, force: bool) -> bool {
        self.send(ToUi::AskForConfirm { prompt, force });
        match self.recv() {
            FromUi::Approved => true,
            FromUi::Close | FromUi::Answer(_) => false,
        }
    }

    /// Sends a phase start to the render thread.
    ///
    /// - A closed channel panics.
    pub fn phase_started(&self, phase: Phase) {
        self.send(ToUi::PhaseStarted(phase));
    }

    /// Sends the write results to the render thread.
    ///
    /// - A closed channel panics.
    pub fn write_results(&self, applied: Applied) {
        self.send(ToUi::WriteResults(applied));
    }

    /// Sends a failure notice to the render thread.
    ///
    /// - A closed channel panics.
    pub fn error(&self, error: Box<Error>) {
        self.send(ToUi::Error(error));
    }

    /// Returns true if the channel accepts the failure notice.
    pub fn try_error(&self, error: Box<Error>) -> bool {
        self.try_send(ToUi::Error(error))
    }

    /// Opens a background job and returns its reporter.
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

    /// Sends a close notice to the render thread.
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
                if matches!(event, ToUi::AskForConfirm { force: true, .. }) {
                    if replies.send(FromUi::Approved).is_err() {
                        break;
                    }
                } else if matches!(event, ToUi::AskForDetails | ToUi::AskForConfirm { .. }) {
                    if replies.send(FromUi::Close).is_err() {
                        break;
                    }
                } else if matches!(event, ToUi::Prompt(_)) {
                    break;
                }
            }
        });
        Self { to_ui, from_ui }
    }
}

/// A background job reporter over the worker channels.
#[derive(Debug, Clone)]
pub struct TaskReporter {
    /// The worker channel pair.
    handles: EventHandles,
    /// The job behind the updates.
    kind: TaskKind,
}

impl TaskReporter {
    /// Sends a progress update for the job.
    ///
    /// - A closed channel panics.
    pub fn progress(&self, done: usize, total: Option<usize>, row: Option<RowUpdate>) {
        self.handles.send(ToUi::Task {
            kind: self.kind.clone(),
            state: TaskState::Progress { done, total, row },
        });
    }

    /// Sends a closing fact for the job.
    ///
    /// - A closed channel panics.
    pub fn finish(&self, note: TaskNote) {
        self.handles.send(ToUi::Task {
            kind: self.kind.clone(),
            state: TaskState::Finished { note },
        });
    }
}

/// Builds both ends.
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

    /// Sends a reply to the worker.
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
    use confit_differ::{
        diff::Diff,
        view::{View, ViewKind},
    };
    use confit_model::manifest::Manifest;
    use std::sync::Arc;

    /// Builds an empty manifest for a diff side.
    fn empty_manifest() -> Manifest {
        Manifest {
            version: [0, 9, 0],
            documents: Vec::new(),
            hooks: Vec::new(),
        }
    }

    /// Builds an empty diff with no changes.
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
    fn ask_for_confirm_returns_true_on_approval() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        outgoing.send(FromUi::Approved).unwrap();
        assert!(handles.ask_for_confirm(PromptKind::Apply, false));
    }

    #[test]
    fn ask_for_confirm_returns_false_on_close() {
        let (to_ui, _incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        outgoing.send(FromUi::Close).unwrap();
        assert!(!handles.ask_for_confirm(PromptKind::Restore, false));
    }

    #[test]
    fn preview_carries_the_picked_view() {
        let (to_ui, incoming) = crossbeam_channel::unbounded::<ToUi>();
        let (_outgoing, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = EventHandles::new(to_ui, from_ui);
        let diff = empty_diff();
        handles.preview(diff.clone(), ViewKind::Effective);
        assert_eq!(
            incoming.try_recv(),
            Ok(ToUi::Preview {
                diff,
                view: ViewKind::Effective,
            })
        );
    }

    #[test]
    fn silent_approves_force_without_a_reader() {
        let handles = EventHandles::silent();
        assert!(handles.ask_for_confirm(PromptKind::Bundle, true));
    }

    #[test]
    fn silent_closes_plain_confirm_without_a_reader() {
        let handles = EventHandles::silent();
        assert!(!handles.ask_for_confirm(PromptKind::Apply, false));
    }

    #[test]
    fn silent_closes_details_without_a_reader() {
        let handles = EventHandles::silent();
        handles.ask_for_details();
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
