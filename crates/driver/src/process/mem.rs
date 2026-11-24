//! Serves scripted process executions.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use confit_core::error::ContextualCause;

use super::{ProcessCall, ProcessOutput};

/// One scripted process result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedRun {
    /// The process exit code.
    pub code: i32,
    /// The stdout and stderr bytes for the log.
    pub output: Vec<u8>,
}

struct Scripts {
    outcomes: VecDeque<Result<ScriptedRun, ContextualCause>>,
    calls: Vec<ProcessCall>,
}

thread_local! {
    static SCRIPTS: RefCell<Scripts> = const { RefCell::new(Scripts {
        outcomes: VecDeque::new(),
        calls: Vec::new(),
    }) };
}

/// Queues a process outcome for the next execution.
pub fn serve(outcome: Result<ScriptedRun, ContextualCause>) {
    SCRIPTS.with(|cell| cell.borrow_mut().outcomes.push_back(outcome));
}

/// Replaces the waiting queue with the process outcomes.
pub fn script(outcomes: VecDeque<Result<ScriptedRun, ContextualCause>>) {
    SCRIPTS.with(|cell| cell.borrow_mut().outcomes = outcomes);
}

/// Collects the recorded process calls in order.
pub fn calls() -> Vec<ProcessCall> {
    SCRIPTS.with(|cell| cell.borrow().calls.clone())
}

/// Removes every scripted outcome and recorded call.
pub fn clear() {
    SCRIPTS.with(|cell| {
        let mut scripts = cell.borrow_mut();
        scripts.outcomes.clear();
        scripts.calls.clear();
    });
}

/// Writes a scripted outcome to the log and records the call.
pub(super) fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
    log: &std::fs::File,
) -> Result<ProcessOutput, ContextualCause> {
    use std::io::Write as _;

    SCRIPTS.with(|cell| {
        let mut scripts = cell.borrow_mut();
        scripts.calls.push(ProcessCall {
            argv: argv.to_vec(),
            path_dirs: path_dirs.to_vec(),
            timeout_secs,
        });
        match scripts.outcomes.pop_front() {
            Some(Ok(run)) => {
                let mut log = log.try_clone().map_err(ContextualCause::from)?;
                log.write_all(&run.output).map_err(ContextualCause::from)?;
                Ok(ProcessOutput { code: run.code })
            }
            Some(Err(contextual)) => Err(contextual),
            None => panic!(
                "the scripts hold no process outcome for '{}'",
                argv.join(" ")
            ),
        }
    })
}
