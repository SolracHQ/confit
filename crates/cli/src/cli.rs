//! Argument shapes for the terminal surface.

use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use confit_driver::paths::BaseDirs;

/// The parsed command line for one run.
#[derive(Debug, Parser)]
#[command(name = "confit", version, about = "CaC: configuration as code for one user", long_about = None)]
pub struct Cli {
    /// subcommand to run
    #[command(subcommand)]
    pub command: Command,
    /// logs folder path (default: per-process temp folder)
    #[arg(long)]
    pub logs_dir: Option<PathBuf>,
    /// log level (default: warn)
    #[arg(long, default_value_t = log::LevelFilter::Warn)]
    pub log_level: log::LevelFilter,
}

/// The subcommand the user picks.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// write the evaluated profile to a bundle file or named state
    Bundle(BundleArgs),
    /// preview the profile evaluation with differences against previous state
    Check(CheckArgs),
    /// write the live project onto the machine
    Apply(ApplyArgs),
    /// write one package onto the machine
    Restore(RestoreArgs),
    /// scaffold a profile or rewrite stubs and plugins in the target folder
    Init(InitArgs),
    /// pack one state into a portable bundle file or print its manifest
    Export(ExportArgs),
    /// delete one named state with its unreferenced entries
    Delete(DeleteArgs),
}

/// Default profile file under the project.
pub const DEFAULT_PROFILE: &str = "main.lua";

#[derive(Debug, Args)]
pub struct BundleArgs {
    /// project folder to evaluate
    pub project: PathBuf,
    /// profile file to evaluate (default: main.lua in the project)
    #[arg(short, long)]
    pub profile: Option<PathBuf>,
    /// bundle file or named state to write (`.cb` path or `@name`)
    #[arg(short, long)]
    pub output: PathBuf,
}

#[derive(Debug, Args)]
pub struct CheckArgs {
    /// project folder to evaluate
    pub project: PathBuf,
    /// profile file to evaluate (default: main.lua in the project)
    #[arg(short, long)]
    pub profile: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// project folder to evaluate
    pub project: PathBuf,
    /// profile file to evaluate (default: main.lua in the project)
    #[arg(short, long)]
    pub profile: Option<PathBuf>,
    /// skip the confirmation prompt
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// package to restore (`.cb`, `@name`, or `%N`)
    pub source: PathBuf,
    /// skip the confirmation prompt
    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// folder to scaffold or refresh (default: current folder)
    #[arg()]
    pub dir: Option<PathBuf>,
    /// rewrite the stubs folder under the target
    #[arg(long)]
    pub stubs: bool,
    /// rewrite the core plugins folder under the target
    #[arg(long)]
    pub plugins: bool,
    /// refresh the editor config beside the profile
    #[arg(long)]
    pub lsp: bool,
}

#[derive(Debug, Args)]
pub struct ExportArgs {
    /// state to export (`%N` history or `@name`, default: applied state)
    pub picker: Option<String>,
    /// write the bundle to a file (default: state name with `.cb`)
    #[arg(short, long, conflicts_with = "manifest")]
    pub output: Option<PathBuf>,
    /// print the manifest as pretty JSON
    #[arg(short, long)]
    pub manifest: bool,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// state to delete (shaped `@name`)
    pub name: String,
}

/// Expands a leading `~` into the home folder.
pub fn expand_tilde(path: &Path) -> PathBuf {
    let Some(raw) = path.to_str() else {
        return path.to_path_buf();
    };
    let rest = if raw == "~" {
        ""
    } else if let Some(stripped) = raw.strip_prefix("~/") {
        stripped
    } else {
        return path.to_path_buf();
    };
    let Some(home) = BaseDirs::new().home else {
        return path.to_path_buf();
    };
    if rest.is_empty() {
        return home;
    }
    home.join(rest)
}

/// Expands tildes in the path arguments of the picked subcommand.
pub fn expand_command(command: &mut Command) {
    fn opt(target: &mut Option<PathBuf>) {
        if let Some(path) = target {
            *path = expand_tilde(path);
        }
    }
    match command {
        Command::Bundle(args) => {
            args.project = expand_tilde(&args.project);
            opt(&mut args.profile);
            args.output = expand_tilde(&args.output);
        }
        Command::Check(args) => {
            args.project = expand_tilde(&args.project);
            opt(&mut args.profile);
        }
        Command::Apply(args) => {
            args.project = expand_tilde(&args.project);
            opt(&mut args.profile);
        }
        Command::Restore(args) => {
            args.source = expand_tilde(&args.source);
        }
        Command::Init(args) => {
            opt(&mut args.dir);
        }
        Command::Export(args) => {
            opt(&mut args.output);
        }
        Command::Delete(_) => {}
    }
}

/// The environment variable that sets the logs folder.
pub const LOGS_DIR_ENV: &str = "CONFIT_LOG_DIR";

/// The log file that the app writes under the logs folder.
pub const APP_LOG_NAME: &str = "confit.log";

/// The folder under the logs folder that holds the hook logs.
pub const HOOKS_DIR_NAME: &str = "hooks";

/// Resolves the logs folder from the flag, the environment, or the default.
pub fn resolve_logs_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return expand_tilde(dir);
    }
    if let Some(dir) = std::env::var_os(LOGS_DIR_ENV) {
        return expand_tilde(Path::new(&dir));
    }
    std::env::temp_dir().join(format!("confit-{}", std::process::id()))
}

/// Resolves an explicit profile or the default against the project.
pub fn resolve_profile(project: &Path, profile: Option<&Path>) -> PathBuf {
    match profile {
        Some(explicit) if explicit.is_absolute() => explicit.to_path_buf(),
        Some(explicit) => project.join(explicit),
        None => project.join(DEFAULT_PROFILE),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn resolve_profile_resolves_the_default_and_explicit_paths() {
        assert_eq!(
            resolve_profile(Path::new("site"), None),
            PathBuf::from("site/main.lua"),
            "the resolver picks the default profile"
        );
        assert_eq!(
            resolve_profile(Path::new("site"), Some(Path::new("laptop.lua"))),
            PathBuf::from("site/laptop.lua"),
            "a relative name joins the project folder"
        );
        assert_eq!(
            resolve_profile(Path::new("site"), Some(Path::new("/elsewhere/laptop.lua"))),
            PathBuf::from("/elsewhere/laptop.lua"),
            "an absolute name keeps its path"
        );
    }

    #[test]
    fn expand_tilde_expands_the_home_folder() {
        let home = BaseDirs::new().home.unwrap();
        assert_eq!(expand_tilde(Path::new("~")), home);
        assert_eq!(
            expand_tilde(Path::new("~/site/main.lua")),
            home.join("site/main.lua")
        );
    }

    #[test]
    fn logs_dir_prefers_the_flag_over_the_default() {
        let home = BaseDirs::new().home.unwrap();
        let out = resolve_logs_dir(Some(Path::new("~/logs")));
        assert_eq!(out, home.join("logs"));
        let out = resolve_logs_dir(None);
        assert!(out.starts_with(std::env::temp_dir()));
    }
}
