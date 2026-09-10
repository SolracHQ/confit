use clap::Parser;

use confit::cli::{Cli, Command};
use confit::error::Result;
use confit::store::{FsPlanWriter, FsStateStore, PlanWriter};

fn main() {
    if let Err(error) = run() {
        eprintln!("confit: {error}");
        std::process::exit(1);
    }
}

/// Route the subcommand; plan payload to stdout or file, summaries to stderr.
fn run() -> Result<()> {
    let cli = Cli::parse();
    match &cli.command {
        Command::Plan(args) => {
            let store = FsStateStore::new(args.state.clone());
            let outcome = confit::cli::run_plan(args, &store)?;
            match &args.output {
                Some(dest) => FsPlanWriter::new().write(&outcome.plan, dest, args.format)?,
                None => println!("{}", args.format.serialize(&outcome.plan)?),
            }
            eprintln!("{}", outcome.summary);
        }
        Command::Status(args) => {
            let store = FsStateStore::new(args.state.clone());
            let summary = confit::cli::run_status(args, &store)?;
            eprintln!("{summary}");
        }
    }
    Ok(())
}
