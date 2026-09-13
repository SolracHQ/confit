use clap::Parser;

use confit::cli::{Cli, Command};
use confit::error::Result;

fn main() {
    if let Err(error) = run() {
        eprintln!("confit: {error}");
        std::process::exit(1);
    }
}

/// Runs the selected subcommand to completion.
///
/// Collision diagnostics log at debug level to a file; the path prints on
/// stderr so a curious user knows where to look.
///
/// # Errors
///
/// Fails with action plus presentation plus log init errors.
fn run() -> Result<()> {
    let cli = Cli::parse();
    let path = confit::services::logging::init_logging(cli.log_file.as_deref())?;
    let result = run_inner(&cli);
    log::logger().flush();
    match result {
        Ok(()) => {
            eprintln!("log: {}", path.display());
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Runs the selected subcommand without log handling.
///
/// # Arguments
///
/// * `cli` - the parsed command holding subcommand plus flags.
///
/// # Errors
///
/// Fails with action plus presentation errors.
fn run_inner(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Plan(args) => {
            let fs = confit::repository::OsFilesystem;
            let outcome =
                confit::actions::run_plan_with_plugins(args, &fs, cli.plugins.as_deref())?;
            for line in confit::presentation::render_warnings(&outcome.warnings) {
                anstream::eprintln!("{line}");
            }
            if args.output.is_none() {
                let payload = confit::presentation::render_plan_payload(&outcome.plan)?;
                println!("{payload}");
            }
            anstream::eprintln!("{}", confit::presentation::render_plan_outcome(&outcome));
        }
        Command::Status(args) => {
            let fs = confit::repository::OsFilesystem;
            let outcome =
                confit::actions::run_status_with_plugins(args, &fs, cli.plugins.as_deref())?;
            for line in confit::presentation::render_warnings(&outcome.warnings) {
                anstream::eprintln!("{line}");
            }
            anstream::eprintln!("{}", confit::presentation::render_status_outcome(&outcome));
        }
    }
    Ok(())
}
