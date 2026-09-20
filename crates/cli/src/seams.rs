//! Seams
//!
//! Injected effects shared by command runners.

use std::io::BufRead;
use std::path::{Path, PathBuf};

use confit_core::error::{Error, Result};
use confit_core::fs::Filesystem;

use crate::fs::OsFs;
use confit_core::plan::{Bundle, DocumentStatus};

use confit_core::progress::{Event, ProgressSender};

use crate::cli::{SharedArgs, resolve_plugins, resolve_root};
use crate::hooks::HookRunner;
use crate::presentation::spinner::{PrintSender, SuspendControl};

/// Host filesystem under sharing by host seams.
static HOST_FS: OsFs = OsFs;

/// Injected effects under one command run.
///
/// Host runs pass stdin plus print senders. Tests pass memory fakes.
///
pub struct Seams<'a> {
    /// Reads plus writes backend, memory under tests.
    pub fs: &'a dyn Filesystem,
    /// Gains the confirmation answer, stdin on the host.
    pub input: &'a mut dyn BufRead,
    /// Gains stderr lines through the renderer, holding `None` for silence.
    pub print: Option<PrintSender>,
    /// Gains engine facts, holding `None` for silence.
    pub progress: Option<ProgressSender>,
    /// Parks widgets across prompts, holding `None` while headless.
    pub suspend: Option<SuspendControl>,
    /// Runs hook subprocesses, holding `None` for the host runner.
    pub hook_runner: Option<&'a dyn HookRunner>,
    /// Gains hook output bytes, holding `None` for no log.
    pub log_file: Option<PathBuf>,
}

impl<'a> Seams<'a> {
    /// Bundles host stdin with the host filesystem.
    ///
    /// # Arguments
    ///
    /// * `input` - the stdin reader under prompting.
    ///
    /// # Returns
    ///
    /// Silent host seams gaining senders through the fields.
    ///
    pub fn host(input: &'a mut dyn BufRead) -> Self {
        Self {
            fs: &HOST_FS,
            input,
            print: None,
            progress: None,
            suspend: None,
            hook_runner: None,
            log_file: None,
        }
    }

    /// Bundles memory fakes for tests.
    ///
    /// # Arguments
    ///
    /// * `fs` - the memory backend under reading plus writing.
    /// * `input` - the answer source under prompting.
    ///
    /// # Returns
    ///
    /// Silent memory seams gaining a sender through chaining.
    ///
    pub fn memory(fs: &'a dyn Filesystem, input: &'a mut dyn BufRead) -> Self {
        Self {
            fs,
            input,
            print: None,
            progress: None,
            suspend: None,
            hook_runner: None,
            log_file: None,
        }
    }

    /// Gains one engine sender while chaining.
    ///
    /// # Arguments
    ///
    /// * `sender` - the facts sender under the run.
    ///
    /// # Returns
    ///
    /// The same seams carrying the sender.
    ///
    pub fn with_progress(mut self, sender: ProgressSender) -> Self {
        self.progress = Some(sender);
        self
    }

    /// Gains one print sender while chaining.
    ///
    /// # Arguments
    ///
    /// * `sender` - the stderr line sender under the run.
    ///
    /// # Returns
    ///
    /// The same seams carrying the sender.
    ///
    pub fn with_print(mut self, sender: PrintSender) -> Self {
        self.print = Some(sender);
        self
    }

    /// Gains one suspend control while chaining.
    ///
    /// # Arguments
    ///
    /// * `control` - the widget control under prompts.
    ///
    /// # Returns
    ///
    /// The same seams carrying the control.
    ///
    pub fn with_suspend(mut self, control: SuspendControl) -> Self {
        self.suspend = Some(control);
        self
    }

    /// Sends one stderr line through the renderer while present.
    ///
    /// # Arguments
    ///
    /// * `line` - the formatted line under printing.
    pub fn print_line(&self, line: String) {
        if let Some(sender) = self.print.as_ref() {
            let _ = sender.send(line);
        }
    }

    /// Prompts for the literal `yes` confirmation.
    ///
    /// Hosted runs ask through the suspend control with widgets
    /// parked. Memory runs read one input line directly.
    ///
    /// # Returns
    ///
    /// True only for the literal `yes` answer.
    ///
    /// # Errors
    ///
    /// Reader failures surface as io errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::seams::Seams;
    /// use confit_core::fs::MemoryFs;
    /// use std::io::Cursor;
    ///
    /// let fs = MemoryFs::new();
    /// let mut input = Cursor::new("yes\n");
    /// let mut seams = Seams::memory(&fs, &mut input);
    /// assert!(matches!(seams.confirm(), Ok(true)));
    /// ```
    pub fn confirm(&mut self) -> Result<bool> {
        if let Some(control) = self.suspend.clone() {
            return control
                .ask("\nApply these changes? Type 'yes' to continue: ")
                .map_err(Error::from);
        }
        log::debug!("prompt waiting for answer");
        let mut answer = String::new();
        let reads = self.input.read_line(&mut answer).map_err(Error::from)?;
        log::debug!("prompt read {reads} bytes");
        Ok(answer.trim() == "yes")
    }

    /// Emits one hashing fact while a sender passes.
    pub fn emit_hashing(&self) {
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::Hashing);
        }
    }

    /// Emits one plan-reading fact while a sender passes.
    pub fn emit_reading_plan(&self, path: &Path) {
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::ReadingPlan {
                path: path.display().to_string(),
            });
        }
    }

    /// Emits one plan-writing fact while a sender passes.
    pub fn emit_writing_plan(&self, documents: usize) {
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::WritingPlan { documents });
        }
    }
}

/// Runs one step and logs its elapsed time at debug.
///
/// Rich log lines carrying per-step context stay inline at
/// their call sites. This covers bare steps only.
///
/// # Arguments
///
/// * `label` - the step name landing in the log line.
/// * `step` - the work under measuring.
///
/// # Returns
///
/// Whatever the step returns.
///
pub fn timed<T>(label: &str, step: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let out = step();
    log::debug!("{label} took {}ms", start.elapsed().as_millis());
    out
}

/// Evaluates one profile through the engine with shared flags.
pub fn evaluate_shared(
    shared: &SharedArgs,
    profile: &Path,
    progress: Option<ProgressSender>,
) -> Result<confit_engine::Evaluation> {
    let root = resolve_root(&shared.root, Some(profile));
    let plugins = resolve_plugins(&root, &shared.plugins);
    confit_engine::evaluate(
        profile,
        confit_engine::EvalOpts {
            root,
            plugins,
            re_fetch: shared.re_fetch,
            cache_dir: None,
            fetcher: None,
            progress,
        },
    )
}

/// Logs finished documents with lifecycle status.
pub fn log_processed(built: &Bundle, previous: &Bundle) {
    for document in &built.manifest.documents {
        let status = match document.status(previous) {
            DocumentStatus::Create => "create",
            DocumentStatus::Update => "update",
            DocumentStatus::Unchanged => "unchanged",
        };
        log::debug!(
            "document processed path={} kind={} status={status}",
            document.path.as_str(),
            document.data.kind().name()
        );
    }
}
