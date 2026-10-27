//! Memory process runs.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use confit_core::error::ErrorContext;

use super::{ProcessCall, ProcessOutput};

struct Scripts {
    outcomes: VecDeque<Result<ProcessOutput, ErrorContext>>,
    calls: Vec<ProcessCall>,
}

thread_local! {
    static SCRIPTS: RefCell<Scripts> = const { RefCell::new(Scripts {
        outcomes: VecDeque::new(),
        calls: Vec::new(),
    }) };
}

/// Script one process outcome for the next run.
pub fn serve(outcome: Result<ProcessOutput, ErrorContext>) {
    SCRIPTS.with(|cell| cell.borrow_mut().outcomes.push_back(outcome));
}

/// Script process outcomes and replace the waiting queue.
pub fn script(outcomes: VecDeque<Result<ProcessOutput, ErrorContext>>) {
    SCRIPTS.with(|cell| cell.borrow_mut().outcomes = outcomes);
}

/// Return the recorded process calls in order.
pub fn calls() -> Vec<ProcessCall> {
    SCRIPTS.with(|cell| cell.borrow().calls.clone())
}

/// Clear every scripted outcome and recorded call.
pub fn clear() {
    SCRIPTS.with(|cell| {
        let mut scripts = cell.borrow_mut();
        scripts.outcomes.clear();
        scripts.calls.clear();
    });
}

/// Serve one scripted outcome and record the call.
pub(super) fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<ProcessOutput, ErrorContext> {
    SCRIPTS.with(|cell| {
        let mut scripts = cell.borrow_mut();
        scripts.calls.push(ProcessCall {
            argv: argv.to_vec(),
            path_dirs: path_dirs.to_vec(),
            timeout_secs,
        });
        match scripts.outcomes.pop_front() {
            Some(outcome) => outcome,
            None => panic!(
                "confit driver: no scripted process outcome for '{}'",
                argv.join(" ")
            ),
        }
    })
}
