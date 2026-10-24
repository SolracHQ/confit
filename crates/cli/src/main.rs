use clap::Parser;

use confit_cli::actions::apply::ApplyRunner;
use confit_cli::cli::{APP_LOG_NAME, Cli, Command, HOOKS_DIR_NAME, apply_root, resolve_logs_dir};
use confit_cli::presentation::spinner::Terminal;
use confit_cli::sinks::Sinks;
use confit_store::{StoreRoots, Stores};

fn main() {
    if let Err(error) = run() {
        eprintln!("confit: {}", error.terminal());
        std::process::exit(1);
    }
}

/// Run the selected subcommand to completion.
fn run() -> confit_cli::error::Result<()> {
    let mut cli = Cli::parse();
    confit_cli::cli::expand_command(&mut cli.command);
    let logs_dir = resolve_logs_dir(cli.logs_dir.as_deref());
    let log_path = logs_dir.join(APP_LOG_NAME);
    let hooks_dir = logs_dir.join(HOOKS_DIR_NAME);
    std::fs::create_dir_all(&hooks_dir)
        .map_err(|error| confit_cli::error::CliError::from_write_io(&hooks_dir, error))?;
    let dispatch = fern::Dispatch::new()
        .format(|out, message, _record| {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|span| span.as_millis())
                .unwrap_or(0);
            out.finish(format_args!("[{stamp}] {message}"))
        })
        .level(cli.log_level)
        .chain(fern::log_file(&log_path).map_err(|error| {
            confit_cli::error::CliError::Unknown {
                context: "log file".to_owned(),
                message: error.to_string(),
            }
        })?);
    // Repeat installs keep the first sink; init runs once per process.
    let _ = dispatch.apply();
    match &cli.command {
        Command::Plan(args) => run_plan(args, &log_path),
        Command::Apply(args) => run_apply(args, &log_path, &hooks_dir),
        Command::Init(args) => run_init(args),
        Command::Export(args) => run_export(args),
        Command::Delete(args) => run_delete(args),
    }
}

/// Run plan with summary output.
fn run_plan(
    args: &confit_cli::cli::PlanArgs,
    log_path: &std::path::Path,
) -> confit_cli::error::Result<()> {
    let terminal = Terminal::new();
    let sender = terminal.sink();
    let mut roots = StoreRoots::standard();
    roots.project_root = args.project.clone();
    let stores = Stores::new(roots, sender.clone());
    let sinks = Sinks {
        progress: sender.clone(),
        print: terminal.print_handle(),
        suspend: terminal.suspend_handle(),
    };
    let prepared = confit_cli::actions::plan::PlanRunner {
        args,
        stores: stores.clone(),
        sinks,
    }
    .execute()?;
    terminal.finish();
    anstream::println!("{}", prepared.text);
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Run apply with preview and prompts.
fn run_apply(
    args: &confit_cli::cli::ApplyArgs,
    log_path: &std::path::Path,
    hooks_dir: &std::path::Path,
) -> confit_cli::error::Result<()> {
    let mut input = std::io::BufReader::new(std::io::stdin());
    let terminal = Terminal::new();
    let sender = terminal.sink();
    let mut roots = StoreRoots::standard();
    roots.project_root = if args.source.is_dir() {
        args.source.clone()
    } else {
        apply_root(&args.source)
    };
    let stores = Stores::new(roots, sender.clone());
    let sinks = Sinks {
        progress: sender.clone(),
        print: terminal.print_handle(),
        suspend: terminal.suspend_handle(),
    };
    let report = match ApplyRunner::from_args(
        args,
        &mut input,
        stores.clone(),
        sinks,
        hooks_dir.to_path_buf(),
    )
    .and_then(ApplyRunner::execute)
    {
        Ok(report) => report,
        Err(error) => {
            terminal.finish();
            anstream::eprintln!("log: {}", log_path.display());
            log::logger().flush();
            return Err(error);
        }
    };
    terminal.finish();
    anstream::println!(
        "applied: {} files, {} removed",
        report.written,
        report.removed
    );
    anstream::println!("restore: confit apply %1");
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Run export and write a bundle file or print its manifest.
fn run_export(args: &confit_cli::cli::ExportArgs) -> confit_cli::error::Result<()> {
    let terminal = Terminal::new();
    let stores = Stores::new(StoreRoots::standard(), terminal.sink());
    let report = confit_cli::actions::export::ExportRunner::run(args, stores, Sinks::default())?;
    if let Some(dest) = report.dest {
        anstream::println!("export: {}", dest.display());
    } else if let Some(manifest) = report.manifest {
        println!("{manifest}");
    }
    Ok(())
}

/// Run delete and drop one named state and unreferenced entries.
fn run_delete(args: &confit_cli::cli::DeleteArgs) -> confit_cli::error::Result<()> {
    let terminal = Terminal::new();
    let stores = Stores::new(StoreRoots::standard(), terminal.sink());
    let report = confit_cli::actions::delete::run(args, stores)?;
    anstream::println!(
        "delete: @{} ({} entries pruned)",
        report.name,
        report.pruned
    );
    Ok(())
}

/// Run init with project scaffolding.
fn run_init(args: &confit_cli::cli::InitArgs) -> confit_cli::error::Result<()> {
    let runner = confit_cli::actions::init::InitRunner { args };
    for outcome in runner.execute()? {
        match outcome {
            confit_cli::actions::init::InitOutcome::Scaffold(report) => {
                anstream::println!("init: {} ({} files)", report.path.display(), report.written);
            }
            confit_cli::actions::init::InitOutcome::Stubs(report) => {
                anstream::println!(
                    "stubs: {} ({} files)",
                    report.path.display(),
                    report.written
                );
            }
            confit_cli::actions::init::InitOutcome::Plugins(report) => {
                anstream::println!(
                    "plugins: {} ({} files)",
                    report.path.display(),
                    report.written
                );
            }
            confit_cli::actions::init::InitOutcome::Lsp(report) => {
                anstream::println!("lsp: {} ({} files)", report.path.display(), report.written);
            }
        }
    }
    Ok(())
}
