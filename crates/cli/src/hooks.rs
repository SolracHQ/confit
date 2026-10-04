//! Runs hooks with captured output.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use confit_driver as driver;

use crate::error::{CliError, Result};

/// Runs one hook argv with extended PATH and a timeout.
///
/// # Errors
///
/// - [`CliError::EmptyArgv`] for empty argv.
/// - [`CliError::Spawn`] for spawn failures.
/// - [`CliError::Wait`] for wait failures.
/// - [`CliError::Timeout`] for timed-out runs.
/// - [`CliError::Output`] for output join failures.
pub fn run(
    argv: &[String],
    path_dirs: &[PathBuf],
    timeout_secs: u64,
) -> Result<driver::process::ProcessOutput> {
    driver::process::run(argv, path_dirs, timeout_secs).map_err(|error| map_error(error, argv))
}

/// Maps one driver process failure onto cli language with argv.
fn map_error(error: driver::process::ProcessError, argv: &[String]) -> CliError {
    let joined = argv.join(" ");
    match error {
        driver::process::ProcessError::Empty => CliError::EmptyArgv,
        driver::process::ProcessError::Spawn { message } => CliError::Spawn {
            argv: joined,
            message,
        },
        driver::process::ProcessError::Wait { message } => CliError::Wait {
            argv: joined,
            message,
        },
        driver::process::ProcessError::Timeout { timeout_secs } => CliError::Timeout {
            argv: joined,
            timeout_secs,
        },
        driver::process::ProcessError::Output => CliError::Output { argv: joined },
    }
}

/// Appends one hook header and captured bytes to the run log.
///
/// - Missing log files start fresh.
///
/// # Errors
///
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for failed log writes.
pub fn append_hook_log(log: &Path, header: &str, output: &[u8]) -> Result<()> {
    let mut sink = driver::fs::append(log).map_err(|error| CliError::from_write_io(log, error))?;
    sink.write_all(header.as_bytes())
        .map_err(|error| CliError::from_write_io(log, error))?;
    sink.write_all(b"\n")
        .map_err(|error| CliError::from_write_io(log, error))?;
    sink.write_all(output)
        .map_err(|error| CliError::from_write_io(log, error))?;
    if !output.ends_with(b"\n") {
        sink.write_all(b"\n")
            .map_err(|error| CliError::from_write_io(log, error))?;
    }
    sink.sync()
        .map_err(|error| CliError::from_write_io(log, error))?;
    Ok(())
}
