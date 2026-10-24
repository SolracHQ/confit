//! Spinner
//! Tty-controlled progress renderer and its sender.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use confit_driver::progress::{Event, ProgressSender};

/// Sender for already formatted stderr lines.
///
/// Lines flow toward the renderer thread for ordered printing.
pub type PrintSender = crossbeam_channel::Sender<String>;

/// Widget pause control for prompts.
///
/// Clones share one draw target. Prompts run inside `suspend`.
#[derive(Debug, Clone)]
pub struct SuspendControl {
    /// Shared draw target for terminal runs.
    multi: indicatif::MultiProgress,
    /// Spinner bar driver.
    spinner: indicatif::ProgressBar,
    /// Print drain handshake for prompts.
    flush_tx: crossbeam_channel::Sender<crossbeam_channel::Sender<()>>,
}

impl SuspendControl {
    /// Run one prompt with widgets parked.
    pub fn suspend<T>(&self, run: impl FnOnce() -> T) -> T {
        self.multi.suspend(run)
    }

    /// Ask one question with widgets parked.
    ///
    /// The answer is true only for the literal `yes`.
    /// Queued prints land before the question.
    ///
    /// # Errors
    ///
    /// Stderr and stdin failures surface as io errors.
    pub fn ask(&self, question: &str) -> std::io::Result<bool> {
        self.flush();
        self.suspend(|| {
            use std::io::{BufRead as _, Write as _};
            let mut stderr = std::io::stderr().lock();
            stderr.write_all(question.as_bytes())?;
            stderr.flush()?;
            drop(stderr);
            log::debug!("prompt waiting for answer");
            let mut line = String::new();
            let got = std::io::stdin().lock().read_line(&mut line)?;
            log::debug!("prompt read {got} bytes");
            // Truth table lives in caller tests.
            Ok(line.trim() == "yes")
        })
    }

    /// Drain queued prints.
    ///
    /// - Missing renderers skip.
    fn flush(&self) {
        let (ack_tx, ack_rx) = crossbeam_channel::bounded::<()>(1);
        if self.flush_tx.send(ack_tx).is_err() {
            return;
        }
        let _ = ack_rx.recv();
    }

    /// Clear the widget.
    fn clear(&self) {
        self.spinner.finish_and_clear();
    }
}

/// One tty-controlled renderer and its senders.
///
/// Thread-owned spinner over facts, prints, and shutdown.
/// Print lines through `println`. Single-line shutdown clear with thread join on finish.
pub struct Terminal {
    /// Fact sender for the renderer thread.
    tx: ProgressSender,
    /// Line sender for the renderer thread.
    print_tx: Option<PrintSender>,
    /// Shutdown signal at finish.
    shutdown: crossbeam_channel::Sender<()>,
    /// Renderer thread join at finish.
    thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Widget control shared with prompts.
    control: Option<SuspendControl>,
    /// Headless sink flag for runs without a stderr terminal.
    headless: bool,
    /// Repeat finish block after the first.
    done: AtomicBool,
}

impl Terminal {
    /// Build one renderer, hidden without a stderr terminal.
    pub fn new() -> Self {
        use std::io::IsTerminal as _;
        let terminal = std::io::stderr().is_terminal();
        let multi = if terminal {
            indicatif::MultiProgress::new()
        } else {
            indicatif::MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden())
        };
        let spinner = multi.add(indicatif::ProgressBar::new_spinner());
        spinner.set_style(Self::spinner_style());
        spinner.set_message("collecting artifacts");
        spinner.enable_steady_tick(Duration::from_millis(80));
        let (tx, rx) = crossbeam_channel::unbounded::<Event>();
        let (print_tx, print_rx) = crossbeam_channel::unbounded::<String>();
        let (flush_tx, flush_rx) = crossbeam_channel::unbounded::<crossbeam_channel::Sender<()>>();
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let control = SuspendControl {
            multi,
            spinner: spinner.clone(),
            flush_tx,
        };
        let thread = std::thread::Builder::new()
            .name("confit-progress".to_string())
            .spawn(move || run(rx, print_rx, flush_rx, shutdown_rx, spinner))
            .ok();
        Self {
            tx,
            print_tx: Some(print_tx),
            shutdown: shutdown_tx,
            thread: std::sync::Mutex::new(thread),
            control: Some(control),
            headless: !terminal,
            done: AtomicBool::new(false),
        }
    }

    /// Return the sender for one run.
    pub fn sink(&self) -> ProgressSender {
        self.tx.clone()
    }

    /// Return the print sender for one run.
    pub fn print_handle(&self) -> Option<PrintSender> {
        if self.headless {
            return None;
        }
        self.print_tx.clone()
    }

    /// Return the suspend control for one run.
    pub fn suspend_handle(&self) -> Option<SuspendControl> {
        if self.headless {
            return None;
        }
        self.control.clone()
    }

    /// Clear the widget and join the renderer at run end.
    pub fn finish(&self) {
        if self.done.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.shutdown.send(());
        let mut guard = match self.thread.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(handle) = guard.take()
            && handle.join().is_err()
            && let Some(control) = self.control.as_ref()
        {
            control.clear();
        }
    }

    /// Build the spinner style with a fallback default.
    fn spinner_style() -> indicatif::ProgressStyle {
        match indicatif::ProgressStyle::with_template("{spinner:.green} {msg}") {
            Ok(style) => style.tick_chars("|/-\\ "),
            Err(_) => indicatif::ProgressStyle::default_spinner(),
        }
    }
}

impl Default for Terminal {
    /// Build one renderer like `new`.
    fn default() -> Self {
        Self::new()
    }
}

/// Loop on facts and prints until shutdown or close, then clear the widget.
fn run(
    rx: crossbeam_channel::Receiver<Event>,
    prints: crossbeam_channel::Receiver<String>,
    flush: crossbeam_channel::Receiver<crossbeam_channel::Sender<()>>,
    shutdown: crossbeam_channel::Receiver<()>,
    spinner: indicatif::ProgressBar,
) {
    loop {
        crossbeam_channel::select! {
            recv(rx) -> fact => {
                match fact {
                    Ok(event) => render(&spinner, event),
                    Err(_) => break,
                }
            }
            recv(prints) -> line => {
                match line {
                    Ok(line) => spinner.println(line),
                    Err(_) => break,
                }
            }
            recv(flush) -> request => {
                match request {
                    Ok(ack) => {
                        while let Ok(line) = prints.try_recv() {
                            spinner.println(line);
                        }
                        let _ = ack.send(());
                    }
                    Err(_) => break,
                }
            }
            recv(shutdown) -> _ => break,
        }
    }
    spinner.finish_and_clear();
}

/// Map one run fact onto the spinner message.
fn render(spinner: &indicatif::ProgressBar, event: Event) {
    log::debug!("event: {event:?}");
    match event {
        Event::FetchStarted { url } => {
            spinner.set_message(format!("collecting artifacts: {url}"));
        }
        Event::FetchProgress {
            url,
            bytes_so_far,
            total,
        } => match total {
            Some(known) => spinner.set_message(format!(
                "collecting artifacts: {url} ({} of {})",
                human_bytes(bytes_so_far),
                human_bytes(known)
            )),
            None => spinner.set_message(format!(
                "collecting artifacts: {url} ({})",
                human_bytes(bytes_so_far)
            )),
        },
        Event::FetchCached { url, bytes } => {
            spinner.set_message(format!(
                "collecting artifacts: {url} ({} cached)",
                human_bytes(bytes_as_u64(bytes))
            ));
        }
        Event::FetchDownloaded { url, bytes } => {
            spinner.set_message(format!(
                "collecting artifacts: {url} ({})",
                human_bytes(bytes_as_u64(bytes))
            ));
        }
        Event::Unpacked { archive, total } => {
            spinner.set_message(format!("collecting artifacts: {archive} ({total} members)"));
        }
        Event::DocumentWritten { path, done, total } => {
            spinner.set_message(format!("writing {done}/{total}: {path}"));
        }
        Event::DocumentRemoved { path, done, total } => {
            spinner.set_message(format!("removing {done}/{total}: {path}"));
        }
        Event::HookRunning {
            position,
            total,
            argv,
        } => {
            spinner.set_message(format!("hook {position} of {total}: {argv}"));
        }
        Event::PoolStored { done } => {
            spinner.set_message(format!("compressing pool entries ({done})"));
        }
        Event::Promoting { blobs } => {
            spinner.set_message(format!("promoting {blobs} pool entries"));
        }
    }
    spinner.tick();
}

/// Render one byte count in human form.
fn human_bytes(bytes: u64) -> String {
    const STEP: f64 = 1024.0;
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut scaled = bytes as f64;
    let mut unit = 0;
    while scaled >= STEP && unit + 1 < UNITS.len() {
        scaled /= STEP;
        unit += 1;
    }
    format!("{scaled:.1} {}", UNITS[unit])
}

/// Convert one byte count into u64.
fn bytes_as_u64(bytes: usize) -> u64 {
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_control() -> (
        SuspendControl,
        crossbeam_channel::Sender<String>,
        crossbeam_channel::Sender<()>,
        std::thread::JoinHandle<()>,
    ) {
        let multi =
            indicatif::MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let spinner = multi.add(indicatif::ProgressBar::new_spinner());
        let (flush_tx, flush_rx) = crossbeam_channel::unbounded::<crossbeam_channel::Sender<()>>();
        let control = SuspendControl {
            multi,
            spinner: spinner.clone(),
            flush_tx,
        };
        let (_event_tx, event_rx) = crossbeam_channel::unbounded::<Event>();
        let (print_tx, print_rx) = crossbeam_channel::unbounded::<String>();
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let handle = std::thread::spawn(move || {
            run(event_rx, print_rx, flush_rx, shutdown_rx, spinner);
        });
        (control, print_tx, shutdown_tx, handle)
    }

    #[test]
    fn flush_acks_queued_and_empty_prints() {
        let (control, print_tx, shutdown_tx, handle) = test_control();
        for line in ["first", "second", "third"] {
            print_tx.send(line.to_owned()).unwrap();
        }
        control.flush();
        let _ = shutdown_tx.send(());
        match handle.join() {
            Ok(()) => {}
            Err(_) => panic!("renderer joins after flush"),
        }
        let (control, _, shutdown_tx, handle) = test_control();
        control.flush();
        let _ = shutdown_tx.send(());
        match handle.join() {
            Ok(()) => {}
            Err(_) => panic!("renderer joins after empty flush"),
        }
    }
}
