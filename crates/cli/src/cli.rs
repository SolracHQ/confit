//! Cli
//! Argument shapes for the terminal surface.

use std::path::{Path, PathBuf};

use clap::{Args, Parser, Subcommand};
use confit_driver::paths::BaseDirs;

/// Configuration as code for one user and plan before apply.
#[derive(Debug, Parser)]
#[command(name = "confit", version, about = "CaC: configuration as code for one user", long_about = None)]
pub struct Cli {
    /// subcommand to run
    #[command(subcommand)]
    pub command: Command,
    /// logs dir path (default: per-process temp dir)
    #[arg(long)]
    pub logs_dir: Option<PathBuf>,
    /// log level (default: warn)
    #[arg(long, default_value_t = log::LevelFilter::Warn)]
    pub log_level: log::LevelFilter,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// preview the profile evaluation with differences against previous state
    Plan(PlanArgs),
    /// confirm the manifest and create every document in place
    Apply(ApplyArgs),
    /// scaffold a profile or rewrite stubs and plugins in the target folder
    Init(InitArgs),
    /// pack one state into a portable bundle file or print its manifest
    Export(ExportArgs),
    /// delete one named state with its unreferenced blobs
    Delete(DeleteArgs),
}

/// Default profile file under the project.
pub const DEFAULT_PROFILE: &str = "main.lua";

/// Evaluate the profile and preview the resulting plan.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// project folder to evaluate
    pub project: PathBuf,
    /// profile file to evaluate (default: main.lua in the project)
    #[arg(short, long)]
    pub profile: Option<PathBuf>,
    /// write the plan bundle to a file (preview only when omitted)
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Apply one source: `.lua` and extensionless paths form a
/// profile, `.cb` forms a bundle file, `@name` forms a named state, `%N`
/// forms history newest-first from one, and folders form a project
/// with `-p` profile choice.
#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// source to apply (`.lua`, `.cb`, `@name`, `%N` or project folder)
    pub source: PathBuf,
    /// profile file to evaluate (default: main.lua in the project)
    #[arg(short, long)]
    pub profile: Option<PathBuf>,
    /// skip the confirmation prompt
    #[arg(long)]
    pub force: bool,
}

/// Scaffold a profile or rewrite stubs and plugins in a folder.
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
    /// write the editor config beside the profile
    #[arg(long)]
    pub lsp: bool,
}

/// Pack one state into a bundle or print its manifest.
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

/// Delete one named state.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// state to delete (shaped `@name`)
    pub name: String,
}

/// Expand one leading `~` against the home folder.
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

/// Expand tildes across every path arg of one command.
pub fn expand_command(command: &mut Command) {
    fn opt(target: &mut Option<PathBuf>) {
        if let Some(path) = target {
            *path = expand_tilde(path);
        }
    }
    match command {
        Command::Plan(args) => {
            args.project = expand_tilde(&args.project);
            if let Some(profile) = args.profile.as_mut() {
                *profile = expand_tilde(profile);
            }
            opt(&mut args.output);
        }
        Command::Apply(args) => {
            args.source = expand_tilde(&args.source);
            if let Some(profile) = args.profile.as_mut() {
                *profile = expand_tilde(profile);
            }
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

/// Log env var filling the gap between the flag and the default.
pub const LOGS_DIR_ENV: &str = "CONFIT_LOG_DIR";

/// App log file name under the logs dir.
pub const APP_LOG_NAME: &str = "confit.log";

/// Hook output folder name under the logs dir.
pub const HOOKS_DIR_NAME: &str = "hooks";

/// Resolve the logs dir from the flag with env and default fallbacks.
///
/// - Explicit flags win over the environment.
/// - `CONFIT_LOG_DIR` fills the gap before the default.
/// - Omitted values use a per-process temp dir.
pub fn resolve_logs_dir(explicit: Option<&Path>) -> PathBuf {
    if let Some(dir) = explicit {
        return expand_tilde(dir);
    }
    if let Some(dir) = std::env::var_os(LOGS_DIR_ENV) {
        return expand_tilde(Path::new(&dir));
    }
    std::env::temp_dir().join(format!("confit-{}", std::process::id()))
}

/// Resolve the plan profile against the project folder.
///
/// - Explicit profiles are absolute or project relative.
/// - Omitted profiles use the default under the project.
pub fn plan_profile(project: &std::path::Path, profile: Option<&std::path::Path>) -> PathBuf {
    match profile {
        Some(explicit) if explicit.is_absolute() => explicit.to_path_buf(),
        Some(explicit) => project.join(explicit),
        None => project.join(DEFAULT_PROFILE),
    }
}

/// Resolve the apply require base from the source parent.
///
/// - Source parent first, current folder fallback for absent parents.
pub fn apply_root(source: &std::path::Path) -> PathBuf {
    source
        .parent()
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn plan_profile_defaults_under_project() {
        assert_eq!(
            plan_profile(Path::new("site"), None),
            PathBuf::from("site/main.lua"),
            "omitted profiles use the default"
        );
    }

    #[test]
    fn plan_profile_joins_relative_names() {
        assert_eq!(
            plan_profile(Path::new("site"), Some(Path::new("laptop.lua"))),
            PathBuf::from("site/laptop.lua"),
            "relative names stay project relative"
        );
    }

    #[test]
    fn plan_profile_keeps_absolute_names() {
        assert_eq!(
            plan_profile(Path::new("site"), Some(Path::new("/elsewhere/laptop.lua"))),
            PathBuf::from("/elsewhere/laptop.lua"),
            "absolute names stay as is"
        );
    }

    #[test]
    fn expand_tilde_bare_home() {
        let home = BaseDirs::new().home.unwrap();
        assert_eq!(expand_tilde(Path::new("~")), home);
    }

    #[test]
    fn expand_tilde_joins_suffix() {
        let home = BaseDirs::new().home.unwrap();
        assert_eq!(
            expand_tilde(Path::new("~/site/main.lua")),
            home.join("site/main.lua")
        );
    }

    #[test]
    fn logs_dir_prefers_explicit_over_env() {
        let home = BaseDirs::new().home.unwrap();
        let out = resolve_logs_dir(Some(Path::new("~/logs")));
        assert_eq!(out, home.join("logs"));
    }

    #[test]
    fn logs_dir_defaults_below_temp() {
        let out = resolve_logs_dir(None);
        assert!(out.starts_with(std::env::temp_dir()));
    }
}
