//! Spinner
//!
//! Tty-gated progress renderer plus its sender.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use confit_core::progress::{Event, ProgressSender};

/// Sender for already formatted stderr lines.
///
/// Lines flow toward the renderer thread for ordered printing.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::spinner::PrintSender;
///
/// let (sender, receiver): (PrintSender, _) = crossbeam_channel::unbounded();
/// assert!(matches!(sender.send("hi".to_string()), Ok(())));
/// ```
pub type PrintSender = crossbeam_channel::Sender<String>;

/// Parks widget painting across prompts.
///
/// Clones share one draw target. Prompts run inside `suspend`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::spinner::Live;
///
/// let live = Live::new();
/// assert!(matches!(live.suspend_handle(), Some(_) | None));
/// live.finish();
/// ```
#[derive(Debug, Clone)]
pub struct SuspendControl {
    /// Shared target holding the spinner on terminal runs.
    multi: indicatif::MultiProgress,
    /// Spinner bar under driving.
    spinner: indicatif::ProgressBar,
}

impl SuspendControl {
    /// Runs one prompt with widgets parked.
    ///
    /// # Arguments
    ///
    /// * `run` - the prompt work under parking.
    ///
    /// # Returns
    ///
    /// Whatever the prompt returns.
    pub fn suspend<T>(&self, run: impl FnOnce() -> T) -> T {
        self.multi.suspend(run)
    }

    /// Asks one question with widgets parked.
    ///
    /// # Arguments
    ///
    /// * `question` - the prompt text under writing to stderr.
    ///
    /// # Returns
    ///
    /// True only for the literal `yes` answer.
    ///
    /// # Errors
    ///
    /// Stderr plus stdin failures surface as io errors.
    pub fn ask(&self, question: &str) -> std::io::Result<bool> {
        self.suspend(|| {
            use std::io::{BufRead as _, Write as _};
            let mut stderr = std::io::stderr().lock();
            stderr.write_all(question.as_bytes())?;
            stderr.flush()?;
            drop(stderr);
            log::debug!("prompt waiting for answer");
            let mut line = String::new();
            let reads = std::io::stdin().lock().read_line(&mut line)?;
            log::debug!("prompt read {reads} bytes");
            // Truth table lives in seams tests.
            Ok(line.trim() == "yes")
        })
    }

    /// Clears the widget.
    fn clear(&self) {
        self.spinner.finish_and_clear();
    }
}

/// One tty-gated renderer plus its senders.
///
/// The thread owns the spinner and selects on facts plus prints
/// plus shutdown. Prints land through `println`. Shutdown clears
/// the single line. Finish signals shutdown, then joins the thread.
///
/// # Examples
///
/// ```rust
/// use confit_cli::presentation::spinner::Live;
///
/// let live = Live::new();
/// assert!(matches!(live.sink(), Some(_) | None));
/// live.finish();
/// ```
pub struct Live {
    /// Facts under sending toward the renderer thread.
    tx: Option<ProgressSender>,
    /// Lines under sending toward the renderer thread.
    print_tx: Option<PrintSender>,
    /// Shutdown signal under sending at finish.
    shutdown: crossbeam_channel::Sender<()>,
    /// Renderer thread under joining at finish.
    thread: std::sync::Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Widget control under sharing with prompts.
    control: Option<SuspendControl>,
    /// Sink gate holding true while stderr is not a terminal.
    headless: bool,
    /// Finish gate holding true after the first finish.
    done: AtomicBool,
}

impl Live {
    /// Builds one renderer hidden while stderr is not a terminal.
    ///
    /// # Returns
    ///
    /// The live renderer for one run.
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
        let control = SuspendControl {
            multi,
            spinner: spinner.clone(),
        };
        let (tx, rx) = crossbeam_channel::unbounded::<Event>();
        let (print_tx, print_rx) = crossbeam_channel::unbounded::<String>();
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let thread = std::thread::Builder::new()
            .name("confit-progress".to_string())
            .spawn(move || run(rx, print_rx, shutdown_rx, spinner))
            .ok();
        Self {
            tx: Some(tx),
            print_tx: Some(print_tx),
            shutdown: shutdown_tx,
            thread: std::sync::Mutex::new(thread),
            control: Some(control),
            headless: !terminal,
            done: AtomicBool::new(false),
        }
    }

    /// Reads the sender for one run.
    ///
    /// # Returns
    ///
    /// The run facts sender, holding `None` off terminal.
    pub fn sink(&self) -> Option<ProgressSender> {
        if self.headless {
            return None;
        }
        self.tx.clone()
    }

    /// Reads the print sender for one run.
    ///
    /// # Returns
    ///
    /// The stderr line sender, holding `None` off terminal.
    pub fn print_handle(&self) -> Option<PrintSender> {
        if self.headless {
            return None;
        }
        self.print_tx.clone()
    }

    /// Reads the suspend control for one run.
    ///
    /// # Returns
    ///
    /// The prompt parking control, holding `None` off terminal.
    pub fn suspend_handle(&self) -> Option<SuspendControl> {
        if self.headless {
            return None;
        }
        self.control.clone()
    }

    /// Clears the widget plus joins the renderer once the run lands.
    ///
    /// Shutdown signals the thread, then joins it. Repeated calls
    /// stay quiet. A dead thread still clears the line.
    pub fn finish(&self) {
        if self.done.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = self.shutdown.send(());
        let mut slot = match self.thread.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(handle) = slot.take()
            && handle.join().is_err()
            && let Some(control) = self.control.as_ref()
        {
            control.clear();
        }
    }

    /// Builds the spinner style with a fallback default.
    fn spinner_style() -> indicatif::ProgressStyle {
        match indicatif::ProgressStyle::with_template("{spinner:.green} {msg}") {
            Ok(style) => style.tick_chars("|/-\\ "),
            Err(_) => indicatif::ProgressStyle::default_spinner(),
        }
    }
}

impl Default for Live {
    /// Builds one renderer like `new`.
    fn default() -> Self {
        Self::new()
    }
}

/// Loops on facts plus prints until shutdown or close, then clears the widget.
fn run(
    rx: crossbeam_channel::Receiver<Event>,
    prints: crossbeam_channel::Receiver<String>,
    shutdown: crossbeam_channel::Receiver<()>,
    spinner: indicatif::ProgressBar,
) {
    let mut writes = 0_usize;
    loop {
        crossbeam_channel::select! {
            recv(rx) -> fact => {
                match fact {
                    Ok(event) => render(&spinner, event, &mut writes),
                    Err(_) => break,
                }
            }
            recv(prints) -> line => {
                match line {
                    Ok(line) => spinner.println(line),
                    Err(_) => break,
                }
            }
            recv(shutdown) -> _ => break,
        }
    }
    spinner.finish_and_clear();
}

/// Maps one run fact onto the spinner message.
fn render(spinner: &indicatif::ProgressBar, event: Event, writes: &mut usize) {
    match event {
        Event::FetchStarted { url } => {
            spinner.set_message(format!("collecting artifacts: {url}"));
        }
        Event::FetchCached { url, bytes } => {
            spinner.set_message(format!(
                "collecting artifacts: {url} ({bytes} bytes, cached)"
            ));
        }
        Event::FetchDownloaded { url, bytes } => {
            spinner.set_message(format!("collecting artifacts: {url} ({bytes} bytes)"));
        }
        Event::Unpacked {
            archive,
            kept,
            total,
        } => {
            spinner.set_message(format!("collecting artifacts: {archive} ({kept}/{total})"));
        }
        Event::PatchApplied { done, total, .. } => {
            spinner.set_message(format!("patching artifacts ({done}/{total})"));
        }
        Event::PatchesStarted { .. } => {
            spinner.set_message("patching artifacts");
        }
        Event::Hashing => {
            spinner.set_message("hashing");
        }
        Event::ReadingPlan { path } => {
            spinner.set_message(format!("reading plan: {path}"));
        }
        Event::WritingPlan { documents } => {
            spinner.set_message(format!("writing plan for {documents} documents"));
        }
        Event::DocumentWritten { path } => {
            *writes += 1;
            spinner.set_message(format!("writing {writes}: {path}"));
        }
        Event::HookRunning {
            position,
            total,
            argv,
        } => {
            spinner.set_message(format!("hook {position} of {total}: {argv}"));
        }
        Event::CompressStarted { .. } => {
            spinner.set_message("compressing blobs");
        }
        Event::BlobCompressed { done, total, .. } => {
            spinner.set_message(format!("compressing blobs ({done}/{total})"));
        }
    }
    spinner.tick();
}
