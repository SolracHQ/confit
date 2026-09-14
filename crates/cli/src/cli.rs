//! Cli
//!
//! Argument shapes for the terminal surface.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Declarative user-space state with plan-before-apply.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::Cli;
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "status", "--profile", "p.lua"]);
/// assert!(matches!(cli, Ok(_)));
/// ```
#[derive(Debug, Parser)]
#[command(name = "confit", version, about = "Declarative user-space state", long_about = None)]
pub struct Cli {
    /// Subcommand selecting the run shape.
    #[command(subcommand)]
    pub command: Command,
}

/// Available subcommands.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "status", "--profile", "p.lua"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Status(_))));
/// ```
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Evaluate the profile, diff against previous state, write the plan.
    Plan(PlanArgs),
    /// Evaluate the profile and diff, keeping the plan in memory.
    Status(StatusArgs),
}

/// Shared run flags carried by both subcommands.
#[derive(Debug, Args)]
pub struct SharedArgs {
    /// Profile Lua file under evaluation.
    #[arg(long)]
    pub profile: PathBuf,
    /// Require resolution base. Defaults to the profile parent.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Previous state file. Omitted means empty previous.
    #[arg(long)]
    pub state: Option<PathBuf>,
    /// Plugin folder shaped `{user}/{name}/plugin.lua`.
    #[arg(long)]
    pub plugins: Option<PathBuf>,
    /// Collision log file. Omitted means a per-process temp path.
    #[arg(long)]
    pub log_file: Option<PathBuf>,
}

/// Arguments for `confit plan`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "plan", "--profile", "p.lua"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Plan(_))));
/// ```
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Shared profile plus seam flags.
    #[command(flatten)]
    pub shared: SharedArgs,
    /// Plan destination. Omitted prints the payload to stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Arguments for `confit status`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "status", "--profile", "p.lua"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Status(_))));
/// ```
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Shared profile plus seam flags.
    #[command(flatten)]
    pub shared: SharedArgs,
}

/// Resolves the require base, defaulting to the profile parent.
///
/// # Arguments
///
/// * `root` - the configured override.
/// * `profile` - the profile path owning the default parent.
///
/// # Returns
///
/// The override, else the profile parent, else the current folder.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::resolve_root;
/// use std::path::Path;
///
/// let root = resolve_root(&None, Path::new("/tmp/profiles/desktop.lua"));
/// assert!(matches!(root.to_str(), Some("/tmp/profiles")));
/// ```
pub fn resolve_root(root: &Option<PathBuf>, profile: &std::path::Path) -> PathBuf {
    if let Some(configured) = root {
        return configured.clone();
    }
    profile
        .parent()
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_root_defaults_to_profile_parent() {
        let root = resolve_root(&None, std::path::Path::new("/tmp/profiles/desktop.lua"));
        assert_eq!(root, PathBuf::from("/tmp/profiles"));
    }
}
