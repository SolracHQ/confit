//! Runs processes on the host.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use confit_core::error::{ContextualCause, FilesystemError, HookRunError};

use super::ProcessOutput;

/// Runs a command with an extended PATH and kills it on timeout.
///
/// - The log holds the partial output after a timeout.
///
/// # Errors
///
/// - [`HookRunError::BinaryUnresolved`] when the argv is empty or the binary is missing.
/// - [`FilesystemError::FilePermissionDenied`] when execution is denied.
/// - [`FilesystemError::FileUnknown`] when the spawn or the wait fails.
/// - [`HookRunError::TimedOut`] when the run times out.
pub(super) fn spawn(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
    log: &std::fs::File,
) -> Result<ProcessOutput, ContextualCause> {
    let Some((program, args)) = argv.split_first() else {
        return Err(ContextualCause::from(HookRunError::BinaryUnresolved));
    };
    let path = extended_path(path_dirs);
    let out = log.try_clone().map_err(io_cause)?;
    let err = log.try_clone().map_err(io_cause)?;
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", path)
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .map_err(io_cause)?;
    let start = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(timeout_secs);
    loop {
        let done = child.try_wait().map_err(io_cause)?;
        match done {
            Some(status) => {
                let code = status.code().unwrap_or(1);
                return Ok(ProcessOutput { code });
            }
            None => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ContextualCause::from(HookRunError::TimedOut));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

/// Converts a spawn or wait fault into a cause.
fn io_cause(error: std::io::Error) -> ContextualCause {
    match error.kind() {
        std::io::ErrorKind::NotFound => ContextualCause::from(HookRunError::BinaryUnresolved),
        std::io::ErrorKind::PermissionDenied => {
            ContextualCause::from(FilesystemError::FilePermissionDenied)
        }
        _ => ContextualCause::from(error),
    }
}

fn extended_path(dirs: &[PathBuf]) -> std::ffi::OsString {
    let mut parts: Vec<PathBuf> = dirs.to_vec();
    if let Some(inherited) = std::env::var_os("PATH") {
        parts.extend(std::env::split_paths(&inherited));
    }
    match std::env::join_paths(parts) {
        Ok(joined) => joined,
        Err(_) => std::ffi::OsString::new(),
    }
}
