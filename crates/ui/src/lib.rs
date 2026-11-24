//! Terminal renderer.

mod backends;
mod shared;

use std::{path::Path, thread::Builder};

use confit_core::error::TerminalResult;
use confit_events::{EventHandles, pair};
use log::logger;

use crate::{
    backends::{inline, none, plain},
    shared::components::style::strExt as _,
};

/// Render backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Prints lines and collects typed answers.
    Plain,
    /// Widget backend with prompts.
    Inline,
    /// Only answers prompts.
    None,
}

/// The terminal UI.
pub struct Ui;

impl Ui {
    /// Runs the closure while the UI runs.
    pub fn init<T>(
        log_path: &Path,
        backend: Backend,
        run: impl FnOnce(EventHandles) -> TerminalResult<T>,
    ) -> TerminalResult<T> {
        let (handles, ui) = pair();
        let sender = handles.clone();
        let renderer = Builder::new()
            .name("confit-progress".to_owned())
            .spawn(move || match backend {
                Backend::Plain => plain::start(ui),
                Backend::Inline => inline::start(ui),
                Backend::None => none::start(ui),
            })
            .ok();
        let outcome = run(handles);
        if let Err(error) = &outcome {
            let _ = sender.try_error(Box::new(error.clone()));
        }
        drop(sender);
        if let Some(renderer) = renderer {
            let _ = renderer.join();
        }
        eprintln!("{}", log_line(log_path));
        logger().flush();
        outcome
    }
}

/// Returns the styled log line.
fn log_line(path: &Path) -> String {
    format!("{}: {}", "Log".dim(), path.display().to_string().header())
}
