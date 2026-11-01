//! Runs processes on the host or a scripted backend.

use std::path::PathBuf;

use confit_core::error::ContextualCause;

#[cfg(not(any(test, feature = "test-support")))]
mod host;
#[cfg(any(test, feature = "test-support"))]
mod mem;

#[cfg(any(test, feature = "test-support"))]
pub use mem::{ScriptedRun, calls, clear, script, serve};

/// The exit code of one process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessOutput {
    /// The process exit code.
    pub code: i32,
}

/// One recorded process call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCall {
    /// The binary and arguments after the binary resolves.
    pub argv: Vec<String>,
    /// The PATH extension folders under the run.
    pub path_dirs: Vec<PathBuf>,
    /// The run limit in seconds.
    pub timeout_secs: u64,
}

/// Runs a command with an extended PATH and a timeout.
///
/// # Errors
///
/// - [`confit_core::error::HookRunError::BinaryUnresolved`] when the argv is empty
///   or the binary is missing.
/// - [`confit_core::error::FilesystemError::FilePermissionDenied`] when execution is denied.
/// - [`confit_core::error::FilesystemError::FileUnknown`] when the spawn or the wait fails.
/// - [`confit_core::error::HookRunError::TimedOut`] when the run times out.
pub fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
    log: &std::fs::File,
) -> Result<ProcessOutput, ContextualCause> {
    #[cfg(any(test, feature = "test-support"))]
    {
        mem::run(argv, path_dirs, timeout_secs, log)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        host::spawn(argv, path_dirs, timeout_secs, log)
    }
}
