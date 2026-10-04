//! Process verbs over host spawns with a memory backend under tests.

use std::path::PathBuf;

#[cfg(not(any(test, feature = "test-support")))]
mod host;
#[cfg(any(test, feature = "test-support"))]
mod mem;

#[cfg(any(test, feature = "test-support"))]
pub use mem::{calls, clear, script, serve};

/// Outcome of one process run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    /// The process exit code.
    pub code: i32,
    /// Captured stdout and stderr bytes in order.
    pub output: Vec<u8>,
}

/// One recorded process run call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCall {
    /// The resolved binary and arguments in order.
    pub argv: Vec<String>,
    /// The PATH extension dirs under the run.
    pub path_dirs: Vec<PathBuf>,
    /// The run cap in seconds.
    pub timeout_secs: u64,
}

/// Process failure shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    /// Empty argv holding nothing.
    Empty,
    /// Failed spawn holding the cause.
    Spawn {
        /// The spawn failure message.
        message: String,
    },
    /// Failed wait holding the cause.
    Wait {
        /// The wait failure message.
        message: String,
    },
    /// Timed-out run holding the cap.
    Timeout {
        /// The run cap in seconds.
        timeout_secs: u64,
    },
    /// Failed output join holding nothing.
    Output,
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "argv reads empty"),
            Self::Spawn { message } => write!(formatter, "{message}"),
            Self::Wait { message } => write!(formatter, "{message}"),
            Self::Timeout { timeout_secs } => {
                write!(formatter, "timed out after {timeout_secs}s")
            }
            Self::Output => write!(formatter, "cannot read output"),
        }
    }
}

impl std::error::Error for ProcessError {}

/// Runs one argv with extended PATH and a timeout.
///
/// # Errors
///
/// - [ProcessError::Empty] for empty argv.
/// - [ProcessError::Spawn] for spawn failures.
/// - [ProcessError::Wait] for wait failures.
/// - [ProcessError::Timeout] for timed-out runs.
/// - [ProcessError::Output] for output join failures.
pub fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<ProcessOutput, ProcessError> {
    #[cfg(any(test, feature = "test-support"))]
    {
        mem::run(argv, path_dirs, timeout_secs)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        host::spawn(argv, path_dirs, timeout_secs)
    }
}
