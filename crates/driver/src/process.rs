//! Process verbs over host spawns with a memory backend under tests.

use std::path::PathBuf;

use confit_core::error::ErrorContext;

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

/// Run one argv with extended PATH and a timeout.
///
/// # Errors
///
/// - [`confit_core::error::HookRunError::BinaryUnresolved`] for empty argv and
///   missing binaries.
/// - [`confit_core::error::FilesystemError::FilePermissionDenied`] for denied execution.
/// - [`confit_core::error::FilesystemError::FileUnknown`] for unknown spawn and wait
///   faults.
/// - [`confit_core::error::HookRunError::TimedOut`] for timed-out runs.
/// - [`confit_core::error::HookRunError::OutputUnreadable`] for output join failures.
pub fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<ProcessOutput, ErrorContext> {
    #[cfg(any(test, feature = "test-support"))]
    {
        mem::run(argv, path_dirs, timeout_secs)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        host::spawn(argv, path_dirs, timeout_secs)
    }
}
