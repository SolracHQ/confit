//! Channel UI for progress, prints, and prompts.

mod protocol;

use indicatif::MultiProgress;
pub use protocol::{Event, FromUi, ToUi};

use std::{io, time::Duration};

use crossbeam_channel::{Receiver, Sender};

/// Handles for worker threads to reach the render thread.
#[derive(Debug, Clone)]
pub struct UiHandles {
    /// Sender for progress, prints, and prompts.
    pub to_ui: Sender<ToUi>,
    /// Receiver for prompt answers.
    pub from_ui: Receiver<FromUi>,
}

impl UiHandles {
    /// Ask one question on the render thread and wait for the answer.
    ///
    /// - Callers hold at most one pending prompt at a time.
    ///
    /// # Errors
    ///
    /// - [`std::io::Error`] for closed channels.
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

/// Live renderer with its worker channels.
pub struct Ui {
    /// Sender for prints and prompts.
    handles: UiHandles,
    /// Renderer join at teardown.
    renderer: Option<std::thread::JoinHandle<()>>,
}

impl Ui {
    /// Start the renderer.
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

    /// Return the worker channels.
    pub fn handles(&self) -> UiHandles {
        self.handles.clone()
    }

    /// Drop the senders and join the thread.
    pub fn finish(self) {
        let Self { handles, renderer } = self;
        drop(handles);
        if let Some(renderer) = renderer {
            let _ = renderer.join();
        }
    }
}

/// Run the render loop until channel close.
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

/// Build the io error for a closed ui channel.
fn send_err_closed() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "ui closed")
}

/// Read one stdin line with widgets parked and send it back.
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

/// Read one stdin line without the trailing newline.
///
/// # Errors
///
/// - [`std::io::Error`] for stderr and stdin failures.
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

/// Map one run fact onto the spinner message.
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

/// Build the spinner style with a fallback default.
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
    fn finish_joins_renderer() {
        let ui = Ui::start();
        ui.finish();
    }

    #[test]
    fn ask_fails_on_closed_channel() {
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
