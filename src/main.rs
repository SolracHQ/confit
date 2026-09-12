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
/// # Errors
///
/// Fails with action plus presentation errors.
fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Plan(args) => {
            let fs = confit::repository::OsFilesystem;
            let outcome = confit::actions::run_plan(args, &fs)?;
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
            let outcome = confit::actions::run_status(args, &fs)?;
            for line in confit::presentation::render_warnings(&outcome.warnings) {
                anstream::eprintln!("{line}");
            }
            anstream::eprintln!("{}", confit::presentation::render_status_outcome(&outcome));
        }
    }
    Ok(())
}
