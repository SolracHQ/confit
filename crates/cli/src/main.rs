use clap::Parser;

use confit_cli::actions::apply::ApplyRunner;
use confit_cli::cli::{APP_LOG_NAME, Cli, Command, HOOKS_DIR_NAME, apply_root, resolve_logs_dir};
use confit_cli::presentation::spinner::Terminal;
use confit_cli::sinks::Sinks;
use confit_core::error::TerminalResult;
use confit_store::{StoreRoots, Stores};

fn main() {
    if let Err(error) = run() {
        let context = error.context();
        let reason = context.log_value();
        let summary = error.summary();
        let mut lines = summary.lines();
        let first = lines.next().unwrap_or_default();
        let rest: Vec<&str> = lines.collect();
        let trace: Vec<&str> = if context.trace.is_empty() {
            match context.detail.as_deref() {
                Some(detail) if rest.len() == 1 && rest[0] == detail => Vec::new(),
                _ => rest,
            }
        } else {
            context.trace.iter().map(String::as_str).collect()
        };
        let mut payload = serde_json::json!({
            "error": first,
            "reason": reason,
        });
        if !trace.is_empty() {
            payload["trace"] = serde_json::json!(trace);
        }
        log::error!(target: "confit:error", "{payload}");
        log::logger().flush();
        eprintln!("error: {}", error.summary());
        let closing = error.closing();
        if !closing.is_empty() {
            eprintln!("{closing}");
        }
        std::process::exit(1);
    }
}

/// Run the selected subcommand to completion.
fn run() -> TerminalResult<()> {
    let mut cli = Cli::parse();
    confit_cli::cli::expand_command(&mut cli.command);
    let logs_dir = resolve_logs_dir(cli.logs_dir.as_deref());
    let log_path = logs_dir.join(APP_LOG_NAME);
    let hooks_dir = logs_dir.join(HOOKS_DIR_NAME);
    if let Err(error) = std::fs::create_dir_all(&hooks_dir) {
        eprintln!(
            "error: cannot create hooks dir '{}': {error}",
            hooks_dir.display()
        );
        std::process::exit(1);
    }
    let log_file = match fern::log_file(&log_path) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("error: cannot open log '{}': {error}", log_path.display());
            std::process::exit(1);
        }
    };
    let dispatch = fern::Dispatch::new()
        .format(|out, message, record| {
            let now = time::OffsetDateTime::now_local()
                .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
            let timestamp = format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
                now.year(),
                now.month() as u8,
                now.day(),
                now.hour(),
                now.minute(),
                now.second(),
                now.millisecond(),
            );
            let level = record.level().to_string();
            let body = message.to_string();
            if record.target() == "confit:error" {
                let mut value = serde_json::from_str::<serde_json::Value>(&body)
                    .unwrap_or_else(|_| serde_json::json!({"message": body}));
                if let Some(fields) = value.as_object_mut() {
                    fields.insert("timestamp".to_owned(), serde_json::Value::String(timestamp));
                    fields.insert("level".to_owned(), serde_json::Value::String(level));
                }
                let text =
                    serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
                out.finish(format_args!("{text}"));
            } else {
                let envelope = serde_json::json!({
                    "timestamp": timestamp,
                    "level": level,
                    "message": body,
                });
                let text = serde_json::to_string_pretty(&envelope)
                    .unwrap_or_else(|_| envelope.to_string());
                out.finish(format_args!("{text}"));
            }
        })
        .level(cli.log_level)
        .chain(log_file);
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
fn run_plan(args: &confit_cli::cli::PlanArgs, log_path: &std::path::Path) -> TerminalResult<()> {
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
) -> TerminalResult<()> {
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
            return Err(error);
        }
    };
    terminal.finish();
    anstream::println!(
        "applied: {} files, {} removed",
        report.written,
        report.removed
    );
    if report.written > 0 || report.removed > 0 {
        anstream::println!("restore: confit apply %1");
    }
    anstream::eprintln!("log: {}", log_path.display());
    log::logger().flush();
    Ok(())
}

/// Run export and write a bundle file or print its manifest.
fn run_export(args: &confit_cli::cli::ExportArgs) -> TerminalResult<()> {
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
fn run_delete(args: &confit_cli::cli::DeleteArgs) -> TerminalResult<()> {
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
fn run_init(args: &confit_cli::cli::InitArgs) -> TerminalResult<()> {
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
