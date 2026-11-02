use clap::Parser;

use std::path::Path;

use confit_cli::actions::bundle::BundleRunner;
use confit_cli::actions::check::CheckRunner;
use confit_cli::actions::export::ExportRunner;
use confit_cli::actions::init::{InitOutcome, InitRunner};
use confit_cli::actions::{apply, delete, restore};
use confit_cli::cli;
use confit_cli::cli::{
    APP_LOG_NAME, ApplyArgs, BundleArgs, CheckArgs, Cli, Command, DeleteArgs, ExportArgs,
    HOOKS_DIR_NAME, InitArgs, RestoreArgs,
};
use confit_core::error::TerminalResult;
use confit_store::{StoreRoots, Stores};
use confit_ui::{Ui, UiHandles};

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

/// Runs the picked subcommand to completion.
fn run() -> TerminalResult<()> {
    let mut cli = Cli::parse();
    cli::expand_command(&mut cli.command);
    let logs_dir = cli::resolve_logs_dir(cli.logs_dir.as_deref());
    let log_path = logs_dir.join(APP_LOG_NAME);
    let hooks_dir = logs_dir.join(HOOKS_DIR_NAME);
    if let Err(error) = std::fs::create_dir_all(&hooks_dir) {
        eprintln!(
            "error: cannot create the hooks folder '{}': {error}",
            hooks_dir.display()
        );
        std::process::exit(1);
    }
    let log_file = match fern::log_file(&log_path) {
        Ok(found) => found,
        Err(error) => {
            eprintln!(
                "error: cannot open the log '{}': {error}",
                log_path.display()
            );
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
    let ui = Ui::start();
    let outcome = match &cli.command {
        Command::Bundle(args) => run_bundle(args, ui.handles()),
        Command::Check(args) => run_check(args, ui.handles(), &log_path),
        Command::Apply(args) => run_apply(args, ui.handles(), &log_path, &hooks_dir),
        Command::Restore(args) => run_restore(args, ui.handles(), &log_path, &hooks_dir),
        Command::Init(args) => run_init(args),
        Command::Export(args) => run_export(args, ui.handles()),
        Command::Delete(args) => run_delete(args, ui.handles()),
    };
    ui.finish();
    if outcome.is_err() {
        anstream::eprintln!("log: {}", log_path.display());
    }
    log::logger().flush();
    outcome
}

/// Writes the bundle and prints the artifact path.
fn run_bundle(args: &BundleArgs, handles: UiHandles) -> TerminalResult<()> {
    let mut roots = StoreRoots::standard();
    roots.project_root = args.project.clone();
    let stores = Stores::new(roots, handles.to_ui.clone());
    let report = BundleRunner {
        args,
        stores: stores.clone(),
        handles,
    }
    .execute()?;
    anstream::println!("bundle: {}", report.text);
    Ok(())
}

/// Checks the project and prints the summary.
fn run_check(args: &CheckArgs, handles: UiHandles, log_path: &Path) -> TerminalResult<()> {
    let mut roots = StoreRoots::standard();
    roots.project_root = args.project.clone();
    let stores = Stores::new(roots, handles.to_ui.clone());
    let prepared = CheckRunner {
        args,
        stores: stores.clone(),
        handles,
    }
    .execute()?;
    anstream::println!("{}", prepared.text);
    anstream::eprintln!("log: {}", log_path.display());
    Ok(())
}

/// Returns whether a previous entry exists.
fn has_previous(stores: &Stores) -> bool {
    stores
        .states()
        .list()
        .map(|entries| entries.len() > 1)
        .unwrap_or(false)
}

/// Applies the plan after the preview and the prompts.
fn run_apply(
    args: &ApplyArgs,
    handles: UiHandles,
    log_path: &Path,
    hooks_dir: &Path,
) -> TerminalResult<()> {
    let mut roots = StoreRoots::standard();
    roots.project_root = args.project.clone();
    let stores = Stores::new(roots, handles.to_ui.clone());
    let report = apply::run(args, stores.clone(), handles, hooks_dir.to_path_buf())?;
    anstream::println!(
        "applied: {} files, {} removed",
        report.written,
        report.removed
    );
    if report.saved > 0 {
        anstream::println!("saved: {} files, confit restore @!", report.saved);
    }
    if (report.written > 0 || report.removed > 0) && has_previous(&stores) {
        anstream::println!("restore: confit restore %1");
    }
    anstream::eprintln!("log: {}", log_path.display());
    Ok(())
}

/// Restores the package after the preview and the prompts.
fn run_restore(
    args: &RestoreArgs,
    handles: UiHandles,
    log_path: &Path,
    hooks_dir: &Path,
) -> TerminalResult<()> {
    let stores = Stores::new(StoreRoots::standard(), handles.to_ui.clone());
    let report = restore::run(args, stores.clone(), handles, hooks_dir.to_path_buf())?;
    anstream::println!(
        "restored: {} files, {} removed",
        report.written,
        report.removed
    );
    if report.saved > 0 {
        anstream::println!("saved: {} files, confit restore @!", report.saved);
    }
    if (report.written > 0 || report.removed > 0) && has_previous(&stores) {
        anstream::println!("restore: confit restore %1");
    }
    anstream::eprintln!("log: {}", log_path.display());
    Ok(())
}

/// Exports the state to a bundle file or prints its manifest.
fn run_export(args: &ExportArgs, handles: UiHandles) -> TerminalResult<()> {
    let stores = Stores::new(StoreRoots::standard(), handles.to_ui.clone());
    let report = ExportRunner::run(args, stores)?;
    if let Some(dest) = report.dest {
        anstream::println!("export: {}", dest.display());
    } else if let Some(manifest) = report.manifest {
        println!("{manifest}");
    }
    Ok(())
}

/// Deletes the named state and its unreferenced entries.
fn run_delete(args: &DeleteArgs, handles: UiHandles) -> TerminalResult<()> {
    let stores = Stores::new(StoreRoots::standard(), handles.to_ui.clone());
    let report = delete::run(args, stores)?;
    anstream::println!(
        "delete: @{} ({} entries pruned)",
        report.name,
        report.pruned
    );
    Ok(())
}

/// Scaffolds the project or refreshes the flagged artifacts.
fn run_init(args: &InitArgs) -> TerminalResult<()> {
    let runner = InitRunner { args };
    for outcome in runner.execute()? {
        match outcome {
            InitOutcome::Scaffold(report) => {
                anstream::println!("init: {} ({} files)", report.path.display(), report.written);
            }
            InitOutcome::Stubs(report) => {
                anstream::println!(
                    "stubs: {} ({} files)",
                    report.path.display(),
                    report.written
                );
            }
            InitOutcome::Plugins(report) => {
                anstream::println!(
                    "plugins: {} ({} files)",
                    report.path.display(),
                    report.written
                );
            }
            InitOutcome::Lsp(report) => {
                anstream::println!("lsp: {} ({} files)", report.path.display(), report.written);
            }
        }
    }
    Ok(())
}
