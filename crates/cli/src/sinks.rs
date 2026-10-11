//! Output senders and shared helpers.

use std::io::BufRead;

use crate::error::{CliError, Result};

use confit_driver::progress::ProgressSender;

use crate::presentation::spinner::{PrintSender, SuspendControl};

/// Output senders for one command run.
#[derive(Debug, Clone)]
pub struct Sinks {
    /// Stderr line sender, silent when unset.
    pub print: Option<PrintSender>,
    /// Engine fact sender.
    pub progress: ProgressSender,
    /// Widget pause control for prompts, empty while headless.
    pub suspend: Option<SuspendControl>,
}

impl Default for Sinks {
    /// Build silent sinks.
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
    /// Send one stderr line through the renderer while present.
    pub fn print_line(&self, line: String) {
        if let Some(sender) = self.print.as_ref() {
            let _ = sender.send(line);
        }
    }

    /// Prompt for the literal `yes` confirmation.
    ///
    /// The answer is true only for the literal `yes`.
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
}
