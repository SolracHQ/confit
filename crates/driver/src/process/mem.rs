//! Memory process runs.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use super::{ProcessCall, ProcessError, ProcessOutput};

struct Scripts {
    outcomes: VecDeque<Result<ProcessOutput, ProcessError>>,
    calls: Vec<ProcessCall>,
}

thread_local! {
    static SCRIPTS: RefCell<Scripts> = const { RefCell::new(Scripts {
        outcomes: VecDeque::new(),
        calls: Vec::new(),
    }) };
}

/// Scripts one process outcome for the next run.
pub fn serve(outcome: Result<ProcessOutput, ProcessError>) {
    SCRIPTS.with(|cell| cell.borrow_mut().outcomes.push_back(outcome));
}

/// Scripts process outcomes replacing the waiting queue.
pub fn script(outcomes: VecDeque<Result<ProcessOutput, ProcessError>>) {
    SCRIPTS.with(|cell| cell.borrow_mut().outcomes = outcomes);
}

/// Collects the recorded process calls in order.
pub fn calls() -> Vec<ProcessCall> {
    SCRIPTS.with(|cell| cell.borrow().calls.clone())
}

/// Clears every scripted outcome and recorded call.
pub fn clear() {
    SCRIPTS.with(|cell| {
        let mut scripts = cell.borrow_mut();
        scripts.outcomes.clear();
        scripts.calls.clear();
    });
}

/// Serves one scripted outcome recording the call.
pub(super) fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<ProcessOutput, ProcessError> {
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
