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

/// Process fault cause.
///
/// One cause covers spawn and wait failures. Unmapped kinds carry
/// their message along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessFault {
    /// Unknown binary.
    UnknownBinary,
    /// Denied execution.
    DeniedExecution,
    /// Unmapped failure with raw text.
    Unknown {
        /// The raw failure text.
        message: String,
    },
}

impl ProcessFault {
    /// Interpret one io kind into its fault.
    pub fn interpret(kind: std::io::ErrorKind) -> Option<Self> {
        match kind {
            std::io::ErrorKind::NotFound => Some(Self::UnknownBinary),
            std::io::ErrorKind::PermissionDenied => Some(Self::DeniedExecution),
            _ => None,
        }
    }

    /// Return the fault cause for display shells.
    ///
    /// Unknown faults echo their raw text.
    pub fn cause(&self) -> &str {
        match self {
            Self::UnknownBinary => "unknown binary",
            Self::DeniedExecution => "denied execution",
            Self::Unknown { message } => message,
        }
    }
}

impl std::fmt::Display for ProcessFault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.cause())
    }
}

/// Process failure shapes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    /// Empty argv.
    Empty,
    /// Failed spawn.
    Spawn {
        /// The spawn fault cause.
        fault: ProcessFault,
    },
    /// Failed wait.
    Wait {
        /// The wait fault cause.
        fault: ProcessFault,
    },
    /// Timed-out run.
    Timeout {
        /// The run cap in seconds.
        timeout_secs: u64,
    },
    /// Failed output join.
    Output,
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "argv is empty"),
            Self::Spawn { fault } => write!(formatter, "{fault}"),
            Self::Wait { fault } => write!(formatter, "{fault}"),
            Self::Timeout { timeout_secs } => {
                write!(formatter, "timed out after {timeout_secs}s")
            }
            Self::Output => write!(formatter, "cannot read output"),
        }
    }
}

impl std::error::Error for ProcessError {}

/// Run one argv with extended PATH and a timeout.
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
