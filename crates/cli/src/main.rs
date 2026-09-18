use clap::Parser;

use confit_cli::cli::{Cli, Command};
use confit_cli::presentation::spinner::Live;

fn main() {
    if let Err(error) = run() {
        match error {
            confit_core::error::Error::Plan(message) => {
                eprintln!("confit: plan error: {message}");
            }
            other => {
                eprintln!("confit: {other}");
            }
        }
        std::process::exit(1);
    }
}

/// Runs the selected subcommand to completion.
///
/// Collision lines land in the log file. The path prints after
/// the summary. Drift notes lead with success, so exit stays 0
/// while drift exists.
fn run() -> confit_core::error::Result<()> {
    let mut cli = Cli::parse();
    confit_cli::cli::expand_command(&mut cli.command);
    if let Some(path) = cli.log_file.as_mut() {
        *path = confit_cli::cli::expand_tilde(path);
    }
    let log_path = match cli.log_file.as_deref() {
        Some(path) => path.to_path_buf(),
        None => std::env::temp_dir().join(format!("confit-{}.log", std::process::id())),
    };
    let dispatch = fern::Dispatch::new()
        .format(|out, message, _record| out.finish(format_args!("{message}")))
        .level(cli.log_level)
        .chain(
            fern::log_file(&log_path)
                .map_err(|error| confit_core::error::Error::Plan(format!("log file: {error}")))?,
        );
    // Repeat installs keep the first sink; init runs once per process.
    let _ = dispatch.apply();
    match &cli.command {
        Command::Plan(args) => {
            let store = args.output.is_none();
            run_plan_like(args, store, &log_path)
        }
        Command::Apply(args) => run_apply(args, &log_path),
        Command::Recover(args) => run_recover(args, &log_path),
        Command::Init(args) => run_init(args),
        Command::Export(args) => run_export(args),
        Command::Delete(args) => run_delete(args),
    }
}

/// Runs plan with summary output.
///
/// Plan without a destination stores the payload under tmp and
/// prints the path for later `--plan` reuse.
fn run_plan_like(
    args: &confit_cli::cli::PlanArgs,
    store_tmp: bool,
    log_path: &std::path::Path,
) -> confit_core::error::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    let live = Live::new();
    let mut seams = confit_cli::actions::seams::Seams::host(&mut input, &mut output);
    seams.progress = live.sink();
    let outcome = confit_cli::actions::plan::PlanRunner {
        args,
        store_tmp,
        seams,
    }
    .execute()?;
    live.finish();
    let summary = confit_cli::presentation::summary::Summary {
        built: &outcome.built,
        previous: &outcome.previous,
        drift: &outcome.drift,
        first_run: outcome.first_run,
    };
    anstream::println!("{}", summary.render());
    for line in &outcome.hook_lines {
        anstream::println!("{line}");
    }
    if let Some(stored) = outcome.stored {
        anstream::println!("plan: {}", stored.display());
    }
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Runs apply with preview plus prompts on host seams.
fn run_apply(
    args: &confit_cli::cli::ApplyArgs,
    log_path: &std::path::Path,
) -> confit_core::error::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    let live = Live::new();
    let mut seams = confit_cli::actions::seams::Seams::host(&mut input, &mut output);
    seams.progress = live.sink();
    seams.log_file = Some(log_path.to_path_buf());
    let report = match confit_cli::actions::apply::ApplyRunner::run(args, seams) {
        Ok(report) => report,
        Err(error) => {
            live.finish();
            anstream::eprintln!("log: {}", log_path.display());
            log::logger().flush();
            return Err(error);
        }
    };
    live.finish();
    anstream::println!(
        "applied: {} files, {} removed",
        report.written,
        report.removed
    );
    anstream::println!("previous: {}", report.stored.display());
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Runs recover listing or re-apply on host seams.
fn run_recover(
    args: &confit_cli::cli::RecoverArgs,
    log_path: &std::path::Path,
) -> confit_core::error::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    let live = Live::new();
    let mut seams = confit_cli::actions::seams::Seams::host(&mut input, &mut output);
    seams.progress = live.sink();
    seams.log_file = Some(log_path.to_path_buf());
    let runner = confit_cli::actions::recover::RecoverRunner { args, seams };
    let report = match runner.execute() {
        Ok(report) => report,
        Err(error) => {
            live.finish();
            anstream::eprintln!("log: {}", log_path.display());
            log::logger().flush();
            return Err(error);
        }
    };
    if let Some(report) = report {
        live.finish();
        anstream::println!(
            "applied: {} files, {} removed",
            report.written,
            report.removed
        );
        anstream::println!("previous: {}", report.stored.display());
    } else {
        live.finish();
    }
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Runs export writing a bundle file or printing its manifest.
fn run_export(args: &confit_cli::cli::ExportArgs) -> confit_core::error::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    let seams = confit_cli::actions::seams::Seams::host(&mut input, &mut output);
    let report = confit_cli::actions::export::ExportRunner::run(args, seams)?;
    if let Some(dest) = report.dest {
        anstream::println!("export: {}", dest.display());
    } else if let Some(manifest) = report.manifest {
        println!("{manifest}");
    }
    Ok(())
}

/// Runs delete dropping one named slot plus orphan blobs.
fn run_delete(args: &confit_cli::cli::DeleteArgs) -> confit_core::error::Result<()> {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let stderr = std::io::stderr();
    let mut output = stderr.lock();
    let seams = confit_cli::actions::seams::Seams::host(&mut input, &mut output);
    let report = confit_cli::actions::delete::DeleteRunner::run(args, seams)?;
    anstream::println!("delete: @{} ({} blobs pruned)", report.name, report.pruned);
    Ok(())
}

/// Runs init with project scaffolding on host seams.
fn run_init(args: &confit_cli::cli::InitArgs) -> confit_core::error::Result<()> {
    let report = confit_cli::actions::init::InitRunner {
        args,
        fs: &confit_cli::fs::OsFs,
    }
    .execute()?;
    anstream::println!(
        "init: {} ({} files)",
        report.profile.display(),
        report.written
    );
    Ok(())
}
