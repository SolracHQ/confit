//! Hooks run
//!
//! Hook subprocess runs with a script registry under test.

use std::path::{Path, PathBuf};
#[cfg(not(test))]
use std::process::{Command, Stdio};

use confit_driver as driver;
use confit_model::error::{Error, Result};

/// Outcome of one hook subprocess run.
///
/// Code holds the process exit code, signal deaths read as 1.
/// Output holds captured stdout and stderr bytes in order.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookRun {
    /// Holds the process exit code.
    pub code: i32,
    /// Holds captured stdout and stderr bytes in order.
    pub output: Vec<u8>,
}

/// Runs one hook argv with extended PATH and a timeout.
///
/// Production spawns the binary directly. Test builds serve
/// scripted outcomes from the thread-local registry instead,
/// so no test ever spawns a subprocess by accident.
///
/// # Arguments
///
/// * `argv` - the resolved binary and arguments in order.
/// * `path_dirs` - the PATH extension dirs for the subprocess alone.
/// * `timeout_secs` - the run cap in seconds.
///
/// # Returns
///
/// The exit code and captured bytes.
///
/// # Errors
///
/// Spawn and wait failures surface as plan errors. Timeouts
/// surface as their own plan error. Unscripted argv fails
/// naming the argv under test.
pub fn run(argv: &[String], path_dirs: &[PathBuf], timeout_secs: u64) -> Result<HookRun> {
    #[cfg(test)]
    {
        registry::run(argv, path_dirs, timeout_secs)
    }
    #[cfg(not(test))]
    {
        spawn(argv, path_dirs, timeout_secs)
    }
}

#[cfg(not(test))]
fn spawn(argv: &[String], path_dirs: &[PathBuf], timeout_secs: u64) -> Result<HookRun> {
    let Some((program, args)) = argv.split_first() else {
        return Err(Error::Plan("hook argv reads empty".to_string()));
    };
    let path = extended_path(path_dirs);
    let mut child = Command::new(program)
        .args(args)
        .env("PATH", path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| Error::Plan(format!("hook '{}' cannot spawn: {error}", argv.join(" "))))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let out_drain = std::thread::spawn(move || drain_pipe(stdout));
    let err_drain = std::thread::spawn(move || drain_pipe(stderr));
    let start = std::time::Instant::now();
    let limit = std::time::Duration::from_secs(timeout_secs);
    loop {
        let done = child.try_wait().map_err(|error| {
            Error::Plan(format!("hook '{}' cannot wait: {error}", argv.join(" ")))
        })?;
        match done {
            Some(status) => {
                let mut output = join_drain(out_drain, argv)?;
                output.extend(join_drain(err_drain, argv)?);
                let code = status.code().unwrap_or(1);
                return Ok(HookRun { code, output });
            }
            None => {
                if start.elapsed() >= limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(Error::Plan(format!(
                        "hook '{}' timed out after {timeout_secs}s",
                        argv.join(" ")
                    )));
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
    }
}

/// Reads one optional pipe into bytes, empty while absent.
#[cfg(not(test))]
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

/// Joins one drain thread into its bytes.
#[cfg(not(test))]
fn join_drain(handle: std::thread::JoinHandle<Vec<u8>>, argv: &[String]) -> Result<Vec<u8>> {
    match handle.join() {
        Ok(bytes) => Ok(bytes),
        Err(_) => Err(Error::Plan(format!(
            "hook '{}' cannot read output",
            argv.join(" ")
        ))),
    }
}

/// Prepends extension dirs to the inherited PATH.
#[cfg(not(test))]
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

/// Appends one hook header and captured bytes to the run log.
///
/// Missing log files start fresh. Backend write failures
/// surface as io errors.
///
/// # Arguments
///
/// * `log` - the log file under appending.
/// * `header` - the hook identity line landing first.
/// * `output` - the captured bytes landing after the header.
///
/// # Returns
///
/// Unit once the bytes land.
///
/// # Errors
///
/// Read and write failures surface as io errors.
pub fn append_hook_log(log: &Path, header: &str, output: &[u8]) -> Result<()> {
    let mut bytes = match driver::read(log) {
        Ok(held) => held,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(Error::from(error)),
    };
    bytes.extend_from_slice(header.as_bytes());
    bytes.extend_from_slice(b"\n");
    bytes.extend_from_slice(output);
    if !output.ends_with(b"\n") {
        bytes.extend_from_slice(b"\n");
    }
    driver::write(log, &bytes).map_err(Error::from)?;
    Ok(())
}

#[cfg(test)]
pub use registry::{HookCall, calls, clear, script, serve};

#[cfg(test)]
pub(crate) mod registry {
    //! Registry
    //!
    //! Thread-local scripted hook outcomes for tests.

    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::path::PathBuf;

    use confit_model::error::Result;

    use super::HookRun;

    /// One recorded hook run call.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct HookCall {
        /// Holds the resolved binary and arguments in order.
        pub argv: Vec<String>,
        /// Holds the PATH extension dirs under the run.
        pub path_dirs: Vec<PathBuf>,
        /// Holds the run cap in seconds under the run.
        pub timeout_secs: u64,
    }

    struct Scripts {
        outcomes: VecDeque<Result<HookRun>>,
        calls: Vec<HookCall>,
    }

    thread_local! {
        static SCRIPTS: RefCell<Scripts> = const { RefCell::new(Scripts {
            outcomes: VecDeque::new(),
            calls: Vec::new(),
        }) };
    }

    /// Scripts one hook outcome for the next run.
    pub fn serve(outcome: Result<HookRun>) {
        SCRIPTS.with(|cell| cell.borrow_mut().outcomes.push_back(outcome));
    }

    /// Scripts hook outcomes replacing the waiting queue.
    pub fn script(outcomes: VecDeque<Result<HookRun>>) {
        SCRIPTS.with(|cell| cell.borrow_mut().outcomes = outcomes);
    }

    /// Reads the recorded hook calls in order.
    pub fn calls() -> Vec<HookCall> {
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
    ) -> Result<HookRun> {
        SCRIPTS.with(|cell| {
            let mut scripts = cell.borrow_mut();
            scripts.calls.push(HookCall {
                argv: argv.to_vec(),
                path_dirs: path_dirs.to_vec(),
                timeout_secs,
            });
            match scripts.outcomes.pop_front() {
                Some(outcome) => outcome,
                None => panic!(
                    "hook registry holds no scripted outcome for '{}'",
                    argv.join(" ")
                ),
            }
        })
    }
}
