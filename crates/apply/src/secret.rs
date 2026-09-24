//! Secret command execution at apply time.

use std::path::PathBuf;

use confit_core::arg::Arg;
use confit_core::error::{Error, Result};
use confit_core::handles::Route;

/// Executes one secret command, returning its stdout bytes.
///
/// Bytes arrive at apply time alone and never persist
/// elsewhere.
///
/// # Arguments
///
/// * `argv` - the command and arguments in order.
/// * `destination` - the destination route naming failures.
/// * `resolve` - the route resolver under expanding.
///
/// # Returns
///
/// Stdout bytes from a successful run.
///
/// # Errors
///
/// Empty commands fail as plan errors. Spawn failures and
/// non-zero statuses fail as plan errors naming the
/// destination.
pub fn run_secret_command(
    argv: &[Arg],
    destination: &Route,
    resolve: &dyn Fn(&Route) -> PathBuf,
) -> Result<Vec<u8>> {
    if argv.is_empty() {
        return Err(Error::Plan(format!(
            "secret '{}': command reads empty",
            destination.display()
        )));
    }
    let display = argv.iter().map(Arg::display).collect::<Vec<_>>().join(" ");
    let expanded: Vec<String> = argv
        .iter()
        .map(|slot| match slot {
            Arg::Text(text) => text.clone(),
            Arg::Route(route) => resolve(route).to_string_lossy().into_owned(),
        })
        .collect();
    let Some((program, args)) = expanded.split_first() else {
        return Err(Error::Plan(format!(
            "secret '{}': command reads empty",
            destination.display()
        )));
    };
    let output = std::process::Command::new(program)
        .args(args)
        .output()
        .map_err(|error| {
            Error::Plan(format!(
                "secret '{}': cannot run '{display}': {error}",
                destination.display(),
            ))
        })?;
    if !output.status.success() {
        return Err(Error::Plan(format!(
            "secret '{}': command '{display}' failed with status {}",
            destination.display(),
            output.status
        )));
    }
    Ok(output.stdout)
}
