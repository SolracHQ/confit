//! Host process runs.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use super::{ProcessError, ProcessFault, ProcessOutput};

/// Run one argv with extended PATH and a timeout that kills late runs.
pub(super) fn spawn(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<ProcessOutput, ProcessError> {
    let Some((program, args)) = argv.split_first() else {
        return Err(ProcessError::Empty);
    };
    let path = extended_path(path_dirs);
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ProcessError::Spawn {
            fault: interpret_fault(error),
        })?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_drain = std::thread::spawn(move || drain_pipe(stdout));
    let err_drain = std::thread::spawn(move || drain_pipe(stderr));
    let start = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(timeout_secs);
    loop {
        let done = child.try_wait().map_err(|error| ProcessError::Wait {
            fault: interpret_fault(error),
        })?;
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
                    return Err(ProcessError::Timeout { timeout_secs });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
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

fn join_drain(handle: std::thread::JoinHandle<Vec<u8>>) -> Result<Vec<u8>, ProcessError> {
    match handle.join() {
        Ok(bytes) => Ok(bytes),
        Err(_) => Err(ProcessError::Output),
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

/// Interpret one io failure into its fault.
fn interpret_fault(error: std::io::Error) -> ProcessFault {
    let kind = error.kind();
    let message = error.to_string();
    match ProcessFault::interpret(kind) {
        Some(fault) => fault,
        None => ProcessFault::Unknown { message },
    }
}
