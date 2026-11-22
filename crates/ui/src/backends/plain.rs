//! Line-based renderer.

use confit_events::{
    UiHandles,
    protocol::{FromUi, PromptKind, ToUi},
};

use crate::shared::{
    components::{
        strings::{
            CANCELLED, CONFIRM_SUFFIX, DETAILS_HEADER, DETAILS_TITLE, PromptKindExt as _, YES,
        },
        style::strExt as _,
    },
    detail::DetailOption,
    execution::{Execution, read_answer},
};

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

    /// Prints the stored detail lines and sends the reply.
    fn on_details(&mut self, handles: &UiHandles) {
        let question = format!("{DETAILS_TITLE}{CONFIRM_SUFFIX}");
        match read_answer(&question) {
            Ok(answer) if answer.trim() == YES => {
                if let Some(view) = self.execution.view() {
                    let picks = DetailOption::collect(view);
                    let lines = DetailOption::render(view, &picks);
                    if !lines.is_empty() {
                        eprintln!("{}", DETAILS_HEADER.header());
                        for line in &lines {
                            eprintln!("{}", line.paint());
                        }
                    }
                }
                handles.send(FromUi::Close);
            }
            Ok(_) => handles.send(FromUi::Close),
            Err(error) => {
                log::debug!("details read failed: {error}");
                handles.send(FromUi::Close);
            }
        }
    }

    /// Asks the per-prompt question and sends the reply.
    fn on_confirm(&mut self, handles: &UiHandles, prompt: PromptKind, force: bool) {
        if force {
            handles.send(FromUi::Approved);
            return;
        }
        let question = format!("{}{CONFIRM_SUFFIX}", prompt.text());
        match read_answer(&question) {
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
            ToUi::Preview { diff, view } => ui.emit(|execution| execution.preview(&diff, view)),
            ToUi::AskForDetails => ui.on_details(&handles),
            ToUi::AskForConfirm { prompt, force } => ui.on_confirm(&handles, prompt, force),
            ToUi::WriteResults(applied) => ui.emit(|execution| execution.write_results(&applied)),
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
