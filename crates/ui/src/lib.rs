//! Worker threads send progress, prints, and prompts to the render thread.

mod protocol;

use indicatif::MultiProgress;
pub use protocol::{Event, FromUi, ToUi};

use std::{io, time::Duration};

use crossbeam_channel::{Receiver, Sender};

/// The channel ends that connect worker threads to the render thread.
#[derive(Debug, Clone)]
pub struct UiHandles {
    /// The end that sends progress, prints, and prompts to the render thread.
    pub to_ui: Sender<ToUi>,
    /// The end that receives prompt answers from the render thread.
    pub from_ui: Receiver<FromUi>,
}

impl UiHandles {
    /// Sends one question to the render thread and returns its answer.
    ///
    /// - Callers hold at most one pending prompt at a time.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] when a channel is closed.
    pub fn ask(&self, prompt: String) -> io::Result<String> {
        if self.to_ui.send(ToUi::Prompt(prompt)).is_err() {
            return Err(send_err_closed());
        }
        match self.from_ui.recv() {
            Ok(FromUi::Answer(answer)) => Ok(answer),
            Err(_) => Err(send_err_closed()),
        }
    }
}

/// The running renderer.
pub struct Ui {
    /// The channels that reach the render thread.
    handles: UiHandles,
    /// The join handle for the render thread, used at teardown.
    renderer: Option<std::thread::JoinHandle<()>>,
}

impl Ui {
    /// Starts the render thread with unbounded worker channels.
    pub fn start() -> Self {
        let (to_ui_tx, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        let (answer_tx, answer_rx) = crossbeam_channel::unbounded::<FromUi>();
        let renderer = std::thread::Builder::new()
            .name("confit-progress".to_owned())
            .spawn(move || run_loop(to_ui_rx, answer_tx))
            .ok();
        Self {
            handles: UiHandles {
                to_ui: to_ui_tx,
                from_ui: answer_rx,
            },
            renderer,
        }
    }

    /// Returns a clone of the worker channels.
    pub fn handles(&self) -> UiHandles {
        self.handles.clone()
    }

    /// Drops the worker channels and waits for the render thread to stop.
    pub fn finish(self) {
        let Self { handles, renderer } = self;
        drop(handles);
        if let Some(renderer) = renderer {
            let _ = renderer.join();
        }
    }
}

/// Renders progress, prints, and prompts until every channel closes.
fn run_loop(rx: Receiver<ToUi>, answers: Sender<FromUi>) {
    use std::io::IsTerminal as _;
    let terminal = io::stderr().is_terminal();
    let multi = if terminal {
        MultiProgress::new()
    } else {
        MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
    };
    let spinner = multi.add(indicatif::ProgressBar::new_spinner());
    spinner.set_style(spinner_style());
    spinner.set_message("collecting artifacts");
    spinner.enable_steady_tick(Duration::from_millis(80));
    loop {
        match rx.recv() {
            Ok(ToUi::Progress(event)) => render(&spinner, event),
            Ok(ToUi::Print(line)) => spinner.println(line),
            Ok(ToUi::Prompt(question)) => respond(&multi, &answers, &question),
            Err(_) => break,
        }
    }
    spinner.finish_and_clear();
}

/// Builds the broken pipe error that a closed ui channel reports.
fn send_err_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "ui closed")
}

/// Parks the widgets, reads one stdin line, and sends the answer back.
fn respond(multi: &MultiProgress, answers: &Sender<FromUi>, question: &str) {
    match multi.suspend(|| read_answer(question)) {
        Ok(text) => {
            let _ = answers.send(FromUi::Answer(text));
        }
        Err(error) => {
            log::debug!("prompt read failed: {error}");
            let _ = answers.send(FromUi::Answer(String::new()));
        }
    }
}

/// Reads one stdin line and trims the trailing newline.
///
/// # Errors
///
/// - [`std::io::Error`] when stderr or stdin fails.
fn read_answer(question: &str) -> io::Result<String> {
    use std::io::{BufRead as _, Write as _};
    let mut stderr = io::stderr().lock();
    stderr.write_all(question.as_bytes())?;
    stderr.flush()?;
    log::debug!("prompt waiting for input");
    let mut line = String::new();
    let got = io::stdin().lock().read_line(&mut line)?;
    log::debug!("prompt read {got} bytes");
    Ok(line.trim_end_matches(['\r', '\n']).to_owned())
}

/// Shows one run fact on the spinner and ticks it.
fn render(spinner: &indicatif::ProgressBar, event: Event) {
    log::debug!("event: {event:?}");
    match event {
        Event::Started { label, .. }
        | Event::Progress { label, .. }
        | Event::Finished { label, .. } => {
            spinner.set_message(label);
        }
    }
    spinner.tick();
}

/// Builds the spinner style, falling back to a plain spinner.
fn spinner_style() -> indicatif::ProgressStyle {
    match indicatif::ProgressStyle::with_template("{spinner:.green} {msg}") {
        Ok(style) => style.tick_chars("|/-\\ "),
        Err(_) => indicatif::ProgressStyle::default_spinner(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_finish_joins_the_renderer() {
        let ui = Ui::start();
        ui.finish();
    }

    #[test]
    fn ask_reports_a_closed_channel() {
        let (to_ui, rx) = crossbeam_channel::unbounded::<ToUi>();
        drop(rx);
        let (_, from_ui) = crossbeam_channel::unbounded::<FromUi>();
        let handles = UiHandles { to_ui, from_ui };
        let error = handles.ask("proceed?".to_owned()).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn run_loop_renders_progress_and_prints() {
        let (to_ui_tx, to_ui_rx) = crossbeam_channel::unbounded::<ToUi>();
        let (answer_tx, _answer_rx) = crossbeam_channel::unbounded::<FromUi>();
        to_ui_tx.send(ToUi::Print("hello".to_owned())).unwrap();
        to_ui_tx
            .send(ToUi::Progress(Event::Started {
                id: "https://example.com/a".to_owned(),
                label: "collecting artifacts: https://example.com/a".to_owned(),
            }))
            .unwrap();
        drop(to_ui_tx);
        run_loop(to_ui_rx, answer_tx);
    }
}
