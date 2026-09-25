//! Sinks
//!
//! Output senders plus shared helpers behind command runners.

use std::io::BufRead;
use std::path::Path;

use confit_model::error::{Error, Result};
use confit_model::plan::DocumentStatus;
use confit_store::Stores;
use confit_store::bundle::Bundle;

use confit_model::progress::{Event, ProgressSender};

use crate::cli::{SharedArgs, resolve_plugins, resolve_root};
use crate::presentation::spinner::{PrintSender, SuspendControl};

/// Output senders behind one command run.
///
/// Silent by default. Host runs attach live senders at the
/// call site through the fields directly.
#[derive(Debug, Clone, Default)]
pub struct Sinks {
    /// Gains stderr lines through the renderer, holding `None` for silence.
    pub print: Option<PrintSender>,
    /// Gains engine facts, holding `None` for silence.
    pub progress: Option<ProgressSender>,
    /// Parks widgets across prompts, holding `None` while headless.
    pub suspend: Option<SuspendControl>,
}

impl Sinks {
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
    /// parked. Headless runs read one input line directly.
    ///
    /// # Arguments
    ///
    /// * `input` - the answer source under prompting.
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
    /// ```rust,no_run
    /// use confit_cli::seams::Sinks;
    /// use std::io::Cursor;
    ///
    /// let sinks = Sinks::default();
    /// let mut input = Cursor::new("yes\n");
    /// assert!(matches!(sinks.confirm(&mut input), Ok(true)));
    /// ```
    pub fn confirm(&self, input: &mut dyn BufRead) -> Result<bool> {
        if let Some(control) = self.suspend.clone() {
            return control
                .ask("\nApply these changes? Type 'yes' to continue: ")
                .map_err(Error::from);
        }
        log::debug!("prompt waiting for answer");
        let mut answer = String::new();
        let reads = input.read_line(&mut answer).map_err(Error::from)?;
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
    pub fn emit_writing_manifest(&self, documents: usize) {
        if let Some(sender) = self.progress.as_ref() {
            let _ = sender.send(Event::WritingManifest { documents });
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
    stores: &Stores,
) -> Result<confit_engine::Evaluation> {
    let root = resolve_root(&shared.root, Some(profile));
    let plugins = resolve_plugins(&root, &shared.plugins);
    confit_engine::evaluate(
        profile,
        confit_engine::EvalOpts {
            root,
            plugins,
            re_fetch: shared.re_fetch,
            stores: Some(stores.clone()),
            progress,
        },
    )
}

/// Logs finished documents with lifecycle status.
pub fn log_processed(built: &Bundle, previous: &Bundle) {
    for document in &built.manifest.documents {
        let status = match document.status(&previous.manifest) {
            DocumentStatus::Create => "create",
            DocumentStatus::Update => "update",
            DocumentStatus::Unchanged => "unchanged",
        };
        log::debug!(
            "document processed path={} kind={} status={status}",
            document.destination.display(),
            document.data.kind().name()
        );
    }
}
