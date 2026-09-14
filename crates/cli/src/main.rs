use clap::Parser;

use confit_cli::cli::{Cli, Command};

fn main() {
    if let Err(error) = run() {
        eprintln!("confit: {error}");
        std::process::exit(1);
    }
}

/// Runs the selected subcommand to completion.
///
/// Collision lines land in the log file. The path prints after
/// the summary. Warnings ride with success, so exit stays 0
/// while warnings exist.
fn run() -> confit_core::error::Result<()> {
    let cli = Cli::parse();
    let (shared, output, payload_to_stdout) = match &cli.command {
        Command::Plan(args) => (&args.shared, args.output.as_deref(), args.output.is_none()),
        Command::Status(args) => (&args.shared, None, false),
    };
    let log_path = logging_init(shared)?;
    let outcome = confit_cli::actions::run(shared, output)?;
    if payload_to_stdout {
        let text = confit_cli::presentation::payload(&outcome.built)?;
        println!("{text}");
    }
    let summary = confit_cli::presentation::render(&outcome.built, &outcome.previous);
    anstream::eprintln!("{summary}");
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Initializes file logging for one run.
fn logging_init(
    shared: &confit_cli::cli::SharedArgs,
) -> confit_core::error::Result<std::path::PathBuf> {
    confit_cli::logging::init(shared.log_file.as_deref())
        .map_err(|error| confit_core::error::Error::Plan(format!("log file: {error}")))
}
