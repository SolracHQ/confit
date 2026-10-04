//! Output senders and shared helpers.

use std::io::BufRead;
use std::path::Path;

use confit_model::document::DocumentStatus;
use confit_model::manifest::Manifest;

use crate::error::{CliError, Result};

use confit_model::progress::{Event, ProgressSender};

use crate::presentation::spinner::{PrintSender, SuspendControl};

/// Output senders for one command run.
#[derive(Debug, Clone)]
pub struct Sinks {
    /// Gains stderr lines through the renderer, holding `None` for silence.
    pub print: Option<PrintSender>,
    /// Gains engine facts.
    pub progress: ProgressSender,
    /// Parks widgets across prompts, holding `None` while headless.
    pub suspend: Option<SuspendControl>,
}

impl Default for Sinks {
    /// Silent sinks.
    fn default() -> Self {
        let (sender, _) = crossbeam_channel::unbounded();
        Self {
            print: None,
            progress: sender,
            suspend: None,
        }
    }
}

impl Sinks {
    /// Sends one stderr line through the renderer while present.
    pub fn print_line(&self, line: String) {
        if let Some(sender) = self.print.as_ref() {
            let _ = sender.send(line);
        }
    }

    /// Prompts for the literal `yes` confirmation.
    ///
    /// True only for the literal `yes` answer.
    ///
    /// # Errors
    ///
    /// - [`CliError::Unknown`] for prompt failures.
    pub fn confirm(&self, input: &mut dyn BufRead) -> Result<bool> {
        if let Some(control) = self.suspend.clone() {
            return control
                .ask("\nApply these changes? Type 'yes' to continue: ")
                .map_err(|error| CliError::Unknown {
                    context: "prompt".to_owned(),
                    message: error.to_string(),
                });
        }
        log::debug!("prompt waiting for answer");
        let mut answer = String::new();
        let reads = input
            .read_line(&mut answer)
            .map_err(|error| CliError::Unknown {
                context: "prompt".to_owned(),
                message: error.to_string(),
            })?;
        log::debug!("prompt read {reads} bytes");
        Ok(answer.trim() == "yes")
    }

    /// Emits one hashing fact while a sender passes.
    pub fn emit_hashing(&self) {
        let _ = self.progress.send(Event::Hashing);
    }

    /// Emits one plan-reading fact while a sender passes.
    pub fn emit_reading_plan(&self, path: &Path) {
        let _ = self.progress.send(Event::ReadingPlan {
            path: path.display().to_string(),
        });
    }

    /// Emits one plan-writing fact while a sender passes.
    pub fn emit_writing_manifest(&self, documents: usize) {
        let _ = self.progress.send(Event::WritingManifest { documents });
    }
}

/// Runs one step and logs its elapsed time at debug.
pub fn timed<T>(label: &str, step: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let out = step();
    log::debug!("{label} took {}ms", start.elapsed().as_millis());
    out
}

/// Logs finished documents with lifecycle status.
pub fn log_processed(built: &Manifest, previous: &Manifest) {
    for document in &built.documents {
        let status = match document.status(previous) {
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
