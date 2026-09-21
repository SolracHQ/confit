//! Hooks run
//!
//! Hook subprocess seam with host and memory backends.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use confit_core::error::{Error, Result};

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

/// Subprocess seam behind hook execution.
///
/// Host runs spawn through the OS. Tests replay scripted
/// outcomes through the fake without spawning.
///
pub trait HookRunner {
    /// Runs one hook argv with extended PATH and a timeout.
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
    /// surface as their own plan error.
    fn run(&self, argv: &[String], path_dirs: &[PathBuf], timeout_secs: u64) -> Result<HookRun>;
}

/// Host hook runner spawning OS processes.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsRunner;

impl HookRunner for OsRunner {
    /// Spawns one hook argv with extended PATH and a timeout.
    ///
    /// The first argv entry runs directly with no shell in
    /// between. Path dirs prepend the inherited PATH for the
    /// subprocess alone. The wait polls on a short sleep loop
    /// and kills past the timeout.
    fn run(&self, argv: &[String], path_dirs: &[PathBuf], timeout_secs: u64) -> Result<HookRun> {
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
            .map_err(|error| {
                Error::Plan(format!("hook '{}' cannot spawn: {error}", argv.join(" ")))
            })?;
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
}

/// Reads one optional pipe into bytes, empty while absent.
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

/// One recorded fake runner call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FakeCall {
    /// Holds the resolved binary and arguments in order.
    pub argv: Vec<String>,
    /// Holds the PATH extension dirs under the run.
    pub path_dirs: Vec<PathBuf>,
    /// Holds the run cap in seconds under the run.
    pub timeout_secs: u64,
}

/// Memory hook runner recording calls and replaying outcomes.
///
/// Tests script one outcome per expected call. Exhausted
/// scripts panic, so missing calls surface loudly.
///
#[derive(Debug, Default)]
pub struct FakeRunner {
    /// Calls seen so far in order.
    calls: std::cell::RefCell<Vec<FakeCall>>,
    /// Scripted outcomes under replay.
    outcomes: std::cell::RefCell<VecDeque<Result<HookRun>>>,
}

impl FakeRunner {
    /// Builds one fake replaying the scripted outcomes.
    ///
    /// # Arguments
    ///
    /// * `outcomes` - the scripted outcomes in call order.
    ///
    /// # Returns
    ///
    /// The fake holding no calls and the script.
    pub fn new(outcomes: VecDeque<Result<HookRun>>) -> Self {
        Self {
            calls: std::cell::RefCell::new(Vec::new()),
            outcomes: std::cell::RefCell::new(outcomes),
        }
    }

    /// Reads the recorded calls in order.
    ///
    /// # Returns
    ///
    /// The calls seen so far.
    pub fn calls(&self) -> Vec<FakeCall> {
        self.calls.borrow().clone()
    }
}

impl HookRunner for FakeRunner {
    /// Records one call and replays the next scripted outcome.
    fn run(&self, argv: &[String], path_dirs: &[PathBuf], timeout_secs: u64) -> Result<HookRun> {
        self.calls.borrow_mut().push(FakeCall {
            argv: argv.to_vec(),
            path_dirs: path_dirs.to_vec(),
            timeout_secs,
        });
        match self.outcomes.borrow_mut().pop_front() {
            Some(outcome) => outcome,
            None => panic!(
                "fake runner holds no scripted outcome for '{}'",
                argv.join(" ")
            ),
        }
    }
}

/// Appends one hook header and captured bytes to the run log.
///
/// Missing log files start fresh. Backend write failures
/// surface as io errors.
///
/// # Arguments
///
/// * `fs` - the backend under writing.
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
pub fn append_hook_log(
    fs: &dyn confit_core::fs::Filesystem,
    log: &Path,
    header: &str,
    output: &[u8],
) -> Result<()> {
    let mut bytes = match fs.read(log) {
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
    fs.write(log, &bytes).map_err(Error::from)?;
    Ok(())
}
