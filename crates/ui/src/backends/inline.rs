//! Inline renderer.

mod bars;
mod detail;

use confit_differ::view::View;
use confit_events::UiHandles;
use confit_events::protocol::{FromUi, TaskKind, TaskState, ToUi};
use inquire::{MultiSelect, Select};

use crate::backends::inline::{bars::Bars, detail::DetailOption};
use crate::shared::{
    components::{
        strings::{CANCELLED, DETAILS, NO, YES},
        style::strExt as _,
    },
    execution::{Execution, read_answer},
    state::{JobProgress, task_label},
};

/// Approval question.
const CONFIRM_TITLE: &str = "Apply these changes?";
/// Row expansion question.
const EXPAND_TITLE: &str = "Do you want more details?";

/// Stateful inline renderer.
struct Inline {
    /// Progress bars.
    bars: Bars,
    /// Shared execution state.
    execution: Execution,
}

impl Inline {
    /// Builds the renderer.
    fn new() -> Self {
        Self {
            bars: Bars::new(),
            execution: Execution::new(),
        }
    }

    /// Handles one UI event.
    fn on(&mut self, event: &ToUi) -> Option<FromUi> {
        match event {
            ToUi::PhaseStarted(phase) => {
                self.emit(|execution| execution.phase(*phase));
                None
            }
            ToUi::Task { kind, state } => {
                self.on_task(kind, state);
                None
            }
            ToUi::ShowAndConfirm { diff, force } => Some(self.on_show(&diff.effective, *force)),
            ToUi::DesiredPreview(diff) => {
                self.emit(|execution| execution.preview(&diff.desired));
                show_details(&self.bars, &diff.desired);
                None
            }
            ToUi::Prompt(question) => Some(ask_text(&self.bars, question)),
            ToUi::WriteResults(applied) => {
                self.emit(|execution| execution.write_results(applied));
                None
            }
            ToUi::Error(error) => {
                self.emit(|execution| execution.failure(error));
                None
            }
            ToUi::Done {
                undo,
                rescued,
                drifted,
            } => {
                self.emit(|execution| execution.close(*undo, *rescued, *drifted));
                None
            }
        }
    }

    /// Handles one task update.
    fn on_task(&mut self, kind: &TaskKind, state: &TaskState) {
        match state {
            TaskState::Started { .. } => {
                self.bars.started(kind, &task_label(kind));
            }
            TaskState::Progress { done, total, .. } => {
                self.bars.progressed(
                    kind,
                    &task_label(kind),
                    JobProgress {
                        done: *done,
                        total: *total,
                    },
                );
            }
            TaskState::Finished { .. } => {
                self.bars.finished(kind);
            }
        }
        self.emit(|execution| execution.task(kind, state));
    }

    /// Prints one preview and collects the approval reply.
    fn on_show(&mut self, view: &View, force: bool) -> FromUi {
        self.emit(|execution| execution.preview(view));
        if force {
            return FromUi::Approved;
        }
        ask_confirm(&self.bars, view)
    }

    /// Prints the lines from one execution update.
    fn emit(&mut self, build: impl FnOnce(&mut Execution) -> Vec<String>) {
        let lines = build(&mut self.execution);
        for line in &lines {
            self.bars.print(line);
        }
    }

    /// Clears the display.
    fn close(&mut self) {
        self.bars.clear();
    }
}

/// Runs the inline UI loop.
pub(crate) fn start(handles: UiHandles) {
    let mut ui = Inline::new();
    while let Some(event) = handles.recv() {
        if let Some(reply) = ui.on(&event) {
            handles.send(reply);
        }
    }
    ui.close();
}

/// Reads one answer.
fn ask_text(bars: &Bars, question: &str) -> FromUi {
    match bars.suspend(|| read_answer(question)) {
        Ok(text) => FromUi::Answer(text),
        Err(error) => {
            log::debug!("prompt read failed: {error}");
            FromUi::Answer(String::new())
        }
    }
}

/// Asks for approval.
fn ask_confirm(bars: &Bars, view: &View) -> FromUi {
    loop {
        match bars.suspend(select_answer) {
            Ok(pick) if pick == YES => return FromUi::Approved,
            Ok(pick) if pick == NO => {
                bars.print(&CANCELLED.dim());
                return FromUi::Close;
            }
            Ok(_) => show_details(bars, view),
            Err(error) => {
                log::debug!("confirm select failed: {error}");
                bars.print(&CANCELLED.dim());
                return FromUi::Close;
            }
        }
    }
}

/// Prompts for approval.
///
/// # Errors
///
/// - [`inquire::InquireError`] when the prompt fails.
fn select_answer() -> Result<String, inquire::InquireError> {
    Select::new(
        CONFIRM_TITLE,
        vec![YES.to_owned(), NO.to_owned(), DETAILS.to_owned()],
    )
    .prompt()
}

/// Prompts for detail options and prints the picked lines.
fn show_details(bars: &Bars, view: &View) {
    let picks = DetailOption::collect(view);
    if picks.is_empty() {
        return;
    }
    let selected = match bars.suspend(|| MultiSelect::new(EXPAND_TITLE, picks).prompt()) {
        Ok(selected) => selected,
        Err(_) => return,
    };
    for line in DetailOption::render(view, &selected) {
        bars.print(&line.paint());
    }
}
