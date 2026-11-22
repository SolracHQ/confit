//! Inline renderer.

mod bars;

use confit_differ::view::View;
use confit_events::{
    UiHandles,
    protocol::{FromUi, PromptKind, TaskKind, TaskState, ToUi},
};
use inquire::{MultiSelect, Select};

use crate::backends::inline::bars::Bars;
use crate::shared::{
    components::{
        strings::{
            CANCELLED, DETAILS, DETAILS_HEADER, DETAILS_TITLE, EXPAND_TITLE, NO,
            PromptKindExt as _, YES,
        },
        style::strExt as _,
    },
    detail::DetailOption,
    execution::{Execution, read_answer},
    state::{JobProgress, task_label},
};

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
            ToUi::Preview { diff, view } => {
                self.emit(|execution| execution.preview(diff, *view));
                None
            }
            ToUi::AskForDetails => Some(self.on_details()),
            ToUi::AskForConfirm { prompt, force } => Some(self.on_confirm(*prompt, *force)),
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

    /// Prints one preview and collects the details reply.
    fn on_details(&mut self) -> FromUi {
        let Some(view) = self.execution.view() else {
            return FromUi::Close;
        };
        ask_details(&self.bars, view)
    }

    /// Prints no preview and collects the approval reply.
    fn on_confirm(&mut self, prompt: PromptKind, force: bool) -> FromUi {
        if force {
            return FromUi::Approved;
        }
        let Some(view) = self.execution.view() else {
            return FromUi::Close;
        };
        ask_confirm(&self.bars, view, prompt)
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
fn ask_confirm(bars: &Bars, view: &View, prompt: PromptKind) -> FromUi {
    loop {
        match bars.suspend(|| select_answer(prompt)) {
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
fn select_answer(prompt: PromptKind) -> Result<String, inquire::InquireError> {
    Select::new(
        prompt.text(),
        vec![YES.to_owned(), NO.to_owned(), DETAILS.to_owned()],
    )
    .with_starting_cursor(1)
    .prompt()
}

/// Asks whether to expand the stored view.
fn ask_details(bars: &Bars, view: &View) -> FromUi {
    let pick = match bars.suspend(select_details) {
        Ok(pick) => pick,
        Err(error) => {
            log::debug!("details select failed: {error}");
            return FromUi::Close;
        }
    };
    if pick == YES {
        show_details(bars, view);
    }
    FromUi::Close
}

/// Prompts for the details choice.
///
/// # Errors
///
/// - [`inquire::InquireError`] when the prompt fails.
fn select_details() -> Result<String, inquire::InquireError> {
    Select::new(DETAILS_TITLE, vec![YES.to_owned(), NO.to_owned()])
        .with_starting_cursor(1)
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
    let lines = DetailOption::render(view, &selected);
    if lines.is_empty() {
        return;
    }
    bars.print(&DETAILS_HEADER.header());
    for line in &lines {
        bars.print(&line.paint());
    }
}
