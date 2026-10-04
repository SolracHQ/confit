//! Cli
//! Argument shapes for the terminal surface.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// Configuration as code for one user and plan before apply.
#[derive(Debug, Parser)]
#[command(name = "confit", version, about = "CaC: configuration as code for one user", long_about = None)]
pub struct Cli {
    /// Subcommand selecting the run shape.
    #[command(subcommand)]
    pub command: Command,
    /// Log file. Omitted means a per-process temp path.
    #[arg(long)]
    pub log_file: Option<PathBuf>,
    /// Log level. Omitted means warn.
    #[arg(long, default_value_t = log::LevelFilter::Warn)]
    pub log_level: log::LevelFilter,
}

/// Available subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Evaluate the profile, diff against previous state, write the output.
    Plan(PlanArgs),
    /// Preview the manifest, confirm, and create every document in place.
    Apply(ApplyArgs),
    /// Scaffold one profile and stubs in the target folder.
    Init(InitArgs),
    /// Pack one slot into a portable bundle file or print its manifest.
    Export(ExportArgs),
    /// Drop one named slot and its orphaned blobs.
    Delete(DeleteArgs),
}

/// Shared run flags carried by plan and apply.
///
/// Profiles ride positionally. `plan` takes one and `apply` takes a source in
/// every shape.
#[derive(Debug, Args)]
pub struct SharedArgs {
    /// Require resolution base. Defaults to the profile parent.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Plugin folder shaped `{user}/{name}/plugin.lua`. Omitted means
    ///
    /// `{root}/plugins`.
    #[arg(long)]
    pub plugins: Option<PathBuf>,
    /// Forces remote downloads past the sidecar cache.
    #[arg(long = "re-fetch")]
    pub re_fetch: bool,
}

/// Arguments for `confit plan`.
#[derive(Debug, Args)]
pub struct PlanArgs {
    /// Profile Lua file under evaluation.
    pub profile: PathBuf,
    /// Shared seam flags.
    #[command(flatten)]
    pub shared: SharedArgs,
    /// Bundle destination. Omitted writes nothing and previews only.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Arguments for `confit apply`.
///
/// The positional sniffs its shape. `.lua` and extensionless paths read a
/// profile, `.cb` reads a bundle file, `@name` reads a named slot and `%N`
/// reads history newest-first from one.
#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Source under applying.
    pub source: PathBuf,
    /// Shared seam flags.
    #[command(flatten)]
    pub shared: SharedArgs,
    /// Skips the confirmation prompt. Drift still re-prompts.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `confit init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target folder gaining the profile and stubs. Omitted means the current
    ///
    /// folder.
    #[arg(default_value = ".")]
    pub dir: PathBuf,
}

/// Arguments for `confit export`.
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Slot picker holding `%N` history and `@name` slots. Omitted means the
    ///
    /// applied slot.
    pub picker: Option<String>,
    /// Bundle destination. Omitted derives the name from the slot. Gains `.cb`
    ///
    /// unless present.
    #[arg(short, long, conflicts_with = "manifest")]
    pub output: Option<PathBuf>,
    /// Prints pretty manifest JSON to stdout.
    #[arg(short, long)]
    pub manifest: bool,
}

/// Arguments for `confit delete`.
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Named slot under deleting, shaped `@name`.
    pub name: String,
}

/// Expands one leading `~` against the OS home folder.
///
/// Bare `~` and `~/` inputs resolve against the home folder.
///
/// - Other inputs and unknown homes pass through unchanged.
pub fn expand_tilde(path: &std::path::Path) -> PathBuf {
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
    let Some(home) = dirs::home_dir() else {
        return path.to_path_buf();
    };
    if rest.is_empty() {
        return home;
    }
    home.join(rest)
}

/// Expands tildes across every path arg of one command.
pub fn expand_command(command: &mut Command) {
    fn opt(slot: &mut Option<PathBuf>) {
        if let Some(path) = slot {
            *path = expand_tilde(path);
        }
    }
    fn shared(shared: &mut SharedArgs) {
        opt(&mut shared.root);
        opt(&mut shared.plugins);
    }
    match command {
        Command::Plan(args) => {
            args.profile = expand_tilde(&args.profile);
            shared(&mut args.shared);
            opt(&mut args.output);
        }
        Command::Apply(args) => {
            args.source = expand_tilde(&args.source);
            shared(&mut args.shared);
        }
        Command::Init(args) => {
            args.dir = expand_tilde(&args.dir);
        }
        Command::Export(args) => {
            opt(&mut args.output);
        }
        Command::Delete(_) => {}
    }
}

/// Resolves the require base, defaulting to the profile parent.
///
/// - The override, else the profile parent, else the current folder.
pub fn resolve_root(root: &Option<PathBuf>, profile: Option<&std::path::Path>) -> PathBuf {
    if let Some(configured) = root {
        return configured.clone();
    }
    profile
        .and_then(|profile| profile.parent())
        .map_or_else(|| PathBuf::from("."), std::path::Path::to_path_buf)
}

/// Resolves the plugin folder, defaulting under the root.
///
/// - Configured paths win.
pub fn resolve_plugins(root: &std::path::Path, plugins: &Option<PathBuf>) -> PathBuf {
    plugins.clone().unwrap_or_else(|| root.join("plugins"))
}
