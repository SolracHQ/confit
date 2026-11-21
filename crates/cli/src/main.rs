use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use clap::Parser;
use confit_cli::actions::bundle::BundleRunner;
use confit_cli::actions::check::CheckRunner;
use confit_cli::actions::export::ExportRunner;
use confit_cli::actions::init::InitRunner;
use confit_cli::actions::{ActionResult, apply, delete, restore};
use confit_cli::cli::{self, APP_LOG_NAME, Cli, Command, HOOKS_DIR_NAME};
use confit_core::error::{Error, TerminalResult, contextual, kind::ErrorKind};
use confit_model::manifest::Manifest;
use confit_store::{StoreRoots, Stores};
use confit_ui::Ui;

fn main() {
    let mut cli = Cli::parse();
    cli::expand_command(&mut cli.command);
    let logs_dir = cli::resolve_logs_dir(cli.logs_dir.as_deref());
    let log_path = logs_dir.join(APP_LOG_NAME);
    let as_json = cli.json;
    let backend = match cli.ui {
        cli::UiMode::Auto => {
            let stdin_tty = std::io::stdin().is_terminal();
            let stderr_tty = std::io::stderr().is_terminal();
            if !stdin_tty && as_json {
                confit_ui::Backend::None
            } else if !stdin_tty || !stderr_tty {
                confit_ui::Backend::Plain
            } else {
                confit_ui::Backend::Inline
            }
        }
        cli::UiMode::Plain => confit_ui::Backend::Plain,
        cli::UiMode::Inline => confit_ui::Backend::Inline,
        cli::UiMode::None => confit_ui::Backend::None,
    };
    let run_log_path = log_path.clone();
    let outcome: TerminalResult<ActionResult> = Ui::init(&log_path, backend, move |handles| {
        let hooks_dir = setup_logging(&logs_dir, &run_log_path, cli.log_level)?;
        let mut roots = StoreRoots::standard();
        match &cli.command {
            Command::Bundle(args) => roots.project_root = args.project.clone(),
            Command::Check(args) => roots.project_root = args.project.clone(),
            Command::Apply(args) => roots.project_root = args.project.clone(),
            Command::Restore(_) | Command::Init(_) | Command::Export(_) | Command::Delete(_) => {}
        }
        let stores = Stores::new(roots, handles.clone());
        match &cli.command {
            Command::Bundle(args) => BundleRunner {
                args,
                stores,
                handles,
            }
            .execute()
            .map(ActionResult::Artifact),
            Command::Check(args) => CheckRunner {
                args,
                stores,
                handles,
            }
            .execute()
            .map(|report| ActionResult::Check(report.diff)),
            Command::Apply(args) => {
                apply::run(args, stores, handles, hooks_dir).map(ActionResult::Wrote)
            }
            Command::Restore(args) => {
                restore::run(args, stores, handles, hooks_dir).map(ActionResult::Wrote)
            }
            Command::Init(args) => InitRunner { args }.execute().map(ActionResult::Init),
            Command::Export(args) if args.manifest => {
                ExportRunner::run(args, stores).map(ActionResult::Export)
            }
            Command::Export(args) => ExportRunner::run(args, stores).map(ActionResult::Artifact),
            Command::Delete(args) => delete::run(args, stores).map(ActionResult::Delete),
        }
    });
    match outcome {
        Ok(output) => print_output(&output, as_json),
        Err(error) => {
            let payload = error.log_payload();
            log::error!(target: "confit:error", "{payload}");
            if as_json {
                print_json(payload);
            }
            std::process::exit(1);
        }
    }
}

/// Prints the command result to stdout.
fn print_output(output: &ActionResult, as_json: bool) {
    if !as_json {
        if let ActionResult::Export(report) = output {
            print_manifest(&report.manifest);
        }
        return;
    }
    match output {
        ActionResult::Artifact(report) => {
            if let Ok(value) = serde_json::to_value(report) {
                print_json(value);
            }
        }
        ActionResult::Check(diff) => {
            print_json(serde_json::json!(diff));
        }
        ActionResult::Wrote(report) => {
            if let Ok(value) = serde_json::to_value(report) {
                print_json(value);
            }
        }
        ActionResult::Export(report) => {
            print_manifest(&report.manifest);
        }
        ActionResult::Delete(report) => {
            if let Ok(value) = serde_json::to_value(report) {
                print_json(value);
            }
        }
        ActionResult::Init(items) => {
            if items.len() == 1 {
                if let Some(first) = items.first()
                    && let Ok(value) = serde_json::to_value(first)
                {
                    print_json(value);
                }
            } else if let Ok(value) = serde_json::to_value(items) {
                print_json(value);
            }
        }
    }
}

/// Prints one manifest as pretty JSON text to stdout.
fn print_manifest(manifest: &Manifest) {
    match manifest.json() {
        Ok(text) => println!("{text}"),
        Err(_) => print_json(serde_json::json!(manifest)),
    }
}

/// Prints one JSON value as pretty text to stdout.
fn print_json(value: serde_json::Value) {
    match serde_json::to_string_pretty(&value) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("{value}"),
    }
}

/// Creates the hooks folder, then starts the file log.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] when the hooks folder
///   is unreachable or the log file fails to open.
fn setup_logging(
    logs_dir: &Path,
    log_path: &Path,
    log_level: log::LevelFilter,
) -> TerminalResult<PathBuf> {
    let hooks_dir = logs_dir.join(HOOKS_DIR_NAME);
    if let Err(error) = std::fs::create_dir_all(&hooks_dir) {
        return Err(Error::new(
            ErrorKind::WriteHookLog,
            contextual(error).with_fs_path(&hooks_dir),
        ));
    }
    let log_file = match fern::log_file(log_path) {
        Ok(found) => found,
        Err(error) => {
            return Err(Error::new(
                ErrorKind::WriteHookLog,
                contextual(error).with_fs_path(log_path),
            ));
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
        .level(log_level)
        .chain(log_file);
    // Repeat installs keep the first sink; init runs once per process.
    let _ = dispatch.apply();
    Ok(hooks_dir)
}
