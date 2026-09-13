//! Cli
//!
//! Holds the command tree plus argument shapes. Clap derives the parser
//! from them.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Declarative user-space state with plan-before-apply.
#[derive(Debug, Parser)]
#[command(name = "confit", version, about = "Declarative user-space state")]
pub struct Cli {
    /// External plugin folder shaped `{user}/{name}/plugin.lua`; empty keeps
    /// embedded defaults only.
    #[arg(long, global = true)]
    pub plugins: Option<PathBuf>,
    /// Collision log file; empty resolves to a per-process path under the
    /// system temp folder.
    #[arg(long, global = true)]
    pub log_file: Option<PathBuf>,
    /// Subcommand to run.
    #[command(subcommand)]
    pub command: Command,
}

/// Available subcommands.
///
/// Holds `plan` and `status`.
/// Later layers add `apply` and `explain`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Evaluate the profile, diff against previous state, write the plan.
    Plan(PlanArgs),
    /// Evaluate the profile and diff, computing the summary in memory.
    Status(StatusArgs),
}

/// Arguments for `confit plan`.
///
/// The evaluated plan plus its summary.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Profile Lua file to evaluate.
    #[arg(long)]
    pub profile: PathBuf,
    /// Confit project root for `require` resolution; defaults to the profile
    /// file's parent directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Destination path for the exported plan; empty prints to stdout.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    /// Previous state file; empty resolves to the empty previous directly.
    #[arg(long)]
    pub state: Option<PathBuf>,
}

/// Arguments for `confit status`.
///
/// The in-memory summary.
#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Profile Lua file to evaluate.
    #[arg(long)]
    pub profile: PathBuf,
    /// Confit project root for `require` resolution; defaults to the profile
    /// file's parent directory.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Previous state file; empty resolves to the empty previous directly.
    #[arg(long)]
    pub state: Option<PathBuf>,
}
