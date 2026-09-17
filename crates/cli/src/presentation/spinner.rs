//! Spinner
//!
//! Tty-gated progress spinner plus its engine sink.

/// One tty-gated spinner plus its engine sink.
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
    /// Spinner under driving, holding `None` off terminal.
    spinner: Option<Spinner>,
    /// Sink gaining facts, holding `None` off terminal.
    sink: Option<confit_engine::ProgressCallback>,
}

impl Live {
    /// Builds one spinner hidden while stderr is not a terminal.
    ///
    /// # Returns
    ///
    /// The live spinner for one run.
    pub fn new() -> Self {
        use std::io::IsTerminal as _;
        if !std::io::stderr().is_terminal() {
            return Self {
                spinner: None,
                sink: None,
            };
        }
        let spinner = Spinner::new();
        let view = spinner.clone();
        let sink: confit_engine::ProgressCallback =
            std::sync::Arc::new(move |event: confit_engine::ProgressEvent| {
                view.render(event);
            });
        Self {
            spinner: Some(spinner),
            sink: Some(sink),
        }
    }

    /// Reads the sink for one run.
    ///
    /// # Returns
    ///
    /// The engine facts receiver, holding `None` off terminal.
    pub fn sink(&self) -> Option<confit_engine::ProgressCallback> {
        self.sink.clone()
    }

    /// Clears the line once the run lands.
    pub fn finish(&self) {
        if let Some(spinner) = self.spinner.as_ref() {
            spinner.finish();
        }
    }
}

impl Default for Live {
    /// Builds one spinner like `new`.
    fn default() -> Self {
        Self::new()
    }
}

/// One spinner bar with its event counters.
#[derive(Debug, Clone)]
struct Spinner {
    /// Bar under driving.
    bar: indicatif::ProgressBar,
    /// Patched entries seen so far.
    patches: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Written documents seen so far.
    writes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Spinner {
    /// Builds one ticking spinner with zeroed counters.
    fn new() -> Self {
        let bar = indicatif::ProgressBar::new_spinner();
        bar.set_style(Self::style());
        // Ticks land per event only, never on a timer. Background
        // redraws would paint over confirmation prompts.
        bar.set_message("collecting artifacts");
        bar.tick();
        Self {
            bar,
            patches: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            writes: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Clears the line once the run lands.
    fn finish(&self) {
        self.bar.finish_and_clear();
    }

    /// Maps one engine fact onto the spinner message.
    fn render(&self, event: confit_engine::ProgressEvent) {
        use std::sync::atomic::Ordering;
        match event {
            confit_engine::ProgressEvent::FetchStarted { url } => {
                self.bar.set_message(format!("collecting artifacts: {url}"));
            }
            confit_engine::ProgressEvent::FetchCached { url, bytes } => {
                self.bar.set_message(format!(
                    "collecting artifacts: {url} ({bytes} bytes, cached)"
                ));
            }
            confit_engine::ProgressEvent::FetchDownloaded { url, bytes } => {
                self.bar
                    .set_message(format!("collecting artifacts: {url} ({bytes} bytes)"));
            }
            confit_engine::ProgressEvent::Unpacked {
                archive,
                kept,
                total,
            } => {
                self.bar
                    .set_message(format!("collecting artifacts: {archive} ({kept}/{total})"));
            }
            confit_engine::ProgressEvent::PatchApplied { .. } => {
                let next = self.patches.fetch_add(1, Ordering::Relaxed) + 1;
                self.bar.set_message(format!("patching artifacts {next}"));
            }
            confit_engine::ProgressEvent::Hashing => {
                self.bar.set_message("hashing");
            }
            confit_engine::ProgressEvent::ReadingPlan { path } => {
                self.bar.set_message(format!("reading plan: {path}"));
            }
            confit_engine::ProgressEvent::WritingPlan { documents } => {
                self.bar
                    .set_message(format!("writing plan for {documents} documents"));
            }
            confit_engine::ProgressEvent::DocumentWritten { path } => {
                let next = self.writes.fetch_add(1, Ordering::Relaxed) + 1;
                self.bar.set_message(format!("writing {next}: {path}"));
            }
            confit_engine::ProgressEvent::HookRunning {
                position,
                total,
                argv,
            } => {
                self.bar
                    .set_message(format!("hook {position} of {total}: {argv}"));
            }
        }
        self.bar.tick();
    }

    /// Builds the spinner style with a fallback default.
    fn style() -> indicatif::ProgressStyle {
        match indicatif::ProgressStyle::with_template("{spinner:.green} {msg}") {
            Ok(style) => style.tick_chars("|/-\\ "),
            Err(_) => indicatif::ProgressStyle::default_spinner(),
        }
    }
}
