//! Host process runs.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use confit_core::error::{ErrorContext, FilesystemError, HookRunError};

use super::ProcessOutput;

/// Run one argv with extended PATH and a timeout that kills late runs.
///
/// # Errors
///
/// - [`HookRunError::BinaryUnresolved`] for empty argv and missing binaries.
/// - [`FilesystemError::FilePermissionDenied`] for denied execution.
/// - [`FilesystemError::FileUnknown`] for unknown spawn and wait faults.
/// - [`HookRunError::TimedOut`] for timed-out runs.
/// - [`HookRunError::OutputUnreadable`] for output join failures.
pub(super) fn spawn(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<ProcessOutput, ErrorContext> {
    let Some((program, args)) = argv.split_first() else {
        return Err(ErrorContext::from(HookRunError::BinaryUnresolved));
    };
    let path = extended_path(path_dirs);
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(io_cause)?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_drain = std::thread::spawn(move || drain_pipe(stdout));
    let err_drain = std::thread::spawn(move || drain_pipe(stderr));
    let start = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(timeout_secs);
    loop {
        let done = child.try_wait().map_err(io_cause)?;
        match done {
            Some(status) => {
                let mut output = join_drain(out_drain)?;
                output.extend(join_drain(err_drain)?);
                let code = status.code().unwrap_or(1);
                return Ok(ProcessOutput { code, output });
            }
            None => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ErrorContext::from(HookRunError::TimedOut {
                        secs: timeout_secs,
                    }));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

/// Map one spawn or wait fault to its bare cause.
fn io_cause(error: std::io::Error) -> ErrorContext {
    match error.kind() {
        std::io::ErrorKind::NotFound => ErrorContext::from(HookRunError::BinaryUnresolved),
        std::io::ErrorKind::PermissionDenied => {
            ErrorContext::from(FilesystemError::FilePermissionDenied)
        }
        _ => ErrorContext::from(error),
    }
}

fn drain_pipe<T: std::io::Read>(pipe: Option<T>) -> Vec<u8> {
    match pipe {
        Some(mut pipe) => {
            let mut out = Vec::new();
            let _ = pipe.read_to_end(&mut out);
            out
        }
        None => Vec::new(),
    }
}

fn join_drain(handle: std::thread::JoinHandle<Vec<u8>>) -> Result<Vec<u8>, ErrorContext> {
    match handle.join() {
        Ok(bytes) => Ok(bytes),
        Err(_) => Err(ErrorContext::from(HookRunError::OutputUnreadable)),
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
