//! Output senders and shared helpers.

use std::io::BufRead;

use confit_core::error::{ContextualCause, Terminal, TerminalResult, kind::ErrorKind};
use confit_core::progress::ProgressSender;

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
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ReadReply`] for prompt failures.
    pub fn confirm(&self, input: &mut dyn BufRead) -> TerminalResult<bool> {
        if let Some(control) = self.suspend.clone() {
            return control
                .ask("\nApply these changes? Type 'yes' to continue: ")
                .map_err(ContextualCause::from)
                .terminal(ErrorKind::ReadReply);
        }
        log::debug!("prompt waiting for input");
        let mut line = String::new();
        let got = input
            .read_line(&mut line)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::ReadReply)?;
        log::debug!("prompt read {got} bytes");
        Ok(line.trim() == "yes")
    }
}
