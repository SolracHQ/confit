//! Line-based renderer.

use confit_differ::diff::Diff;
use confit_events::UiHandles;
use confit_events::protocol::{FromUi, ToUi};

use crate::shared::{
    components::{
        strings::{CANCELLED, YES},
        style::strExt as _,
    },
    execution::{Execution, read_answer},
};

/// The typed approval question.
const CONFIRM_TYPED: &str = "Apply these changes? Type 'yes' to continue: ";

/// The line-based render state.
struct Plain {
    /// Shared execution state.
    execution: Execution,
}

impl Plain {
    /// Builds the renderer.
    fn new() -> Self {
        Self {
            execution: Execution::new(),
        }
    }

    /// Prints the approval prompt and sends the reply.
    fn on_show(&mut self, handles: &UiHandles, diff: Diff, force: bool) {
        self.emit(|execution| execution.preview(&diff.effective));
        if force {
            handles.send(FromUi::Approved);
            return;
        }
        match read_answer(CONFIRM_TYPED) {
            Ok(answer) => {
                let reply = confirm_reply(&answer);
                if matches!(reply, FromUi::Close) {
                    eprintln!("{}", CANCELLED.dim());
                }
                handles.send(reply);
            }
            Err(error) => {
                log::debug!("confirm read failed: {error}");
                eprintln!("{}", CANCELLED.dim());
                handles.send(FromUi::Close);
            }
        }
    }

    /// Prints the lines from one execution update.
    fn emit(&mut self, build: impl FnOnce(&mut Execution) -> Vec<String>) {
        let lines = build(&mut self.execution);
        for line in lines {
            eprintln!("{line}");
        }
    }
}

/// Prints the execution and answers its prompts.
pub(crate) fn start(handles: UiHandles) {
    let mut ui = Plain::new();
    while let Some(event) = handles.recv() {
        match event {
            ToUi::PhaseStarted(phase) => ui.emit(|execution| execution.phase(phase)),
            ToUi::Task {
                kind,
                state: update,
            } => ui.emit(|execution| execution.task(&kind, &update)),
            ToUi::DesiredPreview(diff) => ui.emit(|execution| execution.preview(&diff.desired)),
            ToUi::WriteResults(applied) => ui.emit(|execution| execution.write_results(&applied)),
            ToUi::ShowAndConfirm { diff, force } => ui.on_show(&handles, diff, force),
            ToUi::Prompt(question) => match read_answer(&question) {
                Ok(answer) => handles.send(FromUi::Answer(answer)),
                Err(error) => {
                    log::debug!("prompt read failed: {error}");
                    handles.send(FromUi::Answer(String::new()));
                }
            },
            ToUi::Error(error) => ui.emit(|execution| execution.failure(&error)),
            ToUi::Done {
                undo,
                rescued,
                drifted,
            } => ui.emit(|execution| execution.close(undo, rescued, drifted)),
        }
    }
}

/// Maps one typed line to the approval reply.
fn confirm_reply(answer: &str) -> FromUi {
    if answer.trim() == YES {
        FromUi::Approved
    } else {
        FromUi::Close
    }
}
