//! Cli
//!
//! Argument shapes for the terminal surface.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// CaC: configuration as code for one user, with plan-before-apply.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::Cli;
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "plan", "p.lua"]);
/// assert!(matches!(cli, Ok(_)));
/// ```
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
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "plan", "p.lua"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Plan(_))));
/// ```
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
/// Profiles ride positionally: `plan` takes one, `apply`
/// takes a source in every shape.
#[derive(Debug, Args)]
pub struct SharedArgs {
    /// Require resolution base. Defaults to the profile parent.
    #[arg(long)]
    pub root: Option<PathBuf>,
    /// Plugin folder shaped `{user}/{name}/plugin.lua`. Omitted means `{root}/plugins`.
    #[arg(long)]
    pub plugins: Option<PathBuf>,
    /// Forces remote downloads past the sidecar cache.
    #[arg(long = "re-fetch")]
    pub re_fetch: bool,
}

/// Arguments for `confit plan`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "plan", "p.lua"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Plan(_))));
/// ```
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
/// The positional sniffs its shape: `.lua` and extensionless
/// paths read a profile, `.cb` reads a bundle file, `@name`
/// reads a named slot, `%N` reads history newest-first from one.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "apply", "p.lua", "--force"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Apply(_))));
/// ```
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
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "init", "demo"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Init(_))));
/// ```
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target folder gaining the profile and stubs. Omitted means the current folder.
    #[arg(default_value = ".")]
    pub dir: PathBuf,
}

/// Arguments for `confit export`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "export", "@personal", "-o", "backup.cb"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Export(_))));
/// ```
#[derive(Debug, Args)]
pub struct ExportArgs {
    /// Slot picker: `%N` history newest-first from one, `@name` named slot. Omitted means the applied slot.
    pub picker: Option<String>,
    /// Bundle destination. Omitted derives the name from the slot. Gains `.cb` unless present.
    #[arg(short, long, conflicts_with = "manifest")]
    pub output: Option<PathBuf>,
    /// Prints pretty manifest JSON to stdout instead of writing a bundle file.
    #[arg(short, long)]
    pub manifest: bool,
}

/// Arguments for `confit delete`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "delete", "@personal"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Delete(_))));
/// ```
#[derive(Debug, Args)]
pub struct DeleteArgs {
    /// Named slot under deleting, shaped `@name`.
    pub name: String,
}

/// Expands one leading `~` against the OS home folder.
///
/// Bare `~` and `~/` inputs resolve against the home folder.
/// Other inputs pass through unchanged, letting the caller fail
/// with its own context.
///
/// # Returns
///
/// The resolved path for bare tilde inputs with a known home, else
/// the input unchanged.
///
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
/// let root = resolve_root(&None, Some(Path::new("/tmp/profiles/desktop.lua")));
/// assert!(matches!(root.to_str(), Some("/tmp/profiles")));
/// ```
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
/// Configured paths win. Otherwise `{root}/plugins` applies, matching
/// the init scaffold. Missing folders change nothing downstream; the
/// loader skips paths holding no `plugin.lua`.
///
/// # Arguments
///
/// * `root` - the resolved require base.
/// * `plugins` - the configured override.
///
/// # Returns
///
/// The override, else the plugins folder under the root.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::resolve_plugins;
/// use std::path::PathBuf;
///
/// let folder = resolve_plugins(&PathBuf::from("project"), &None);
/// assert!(matches!(folder.to_str(), Some("project/plugins")));
/// let folder = resolve_plugins(&PathBuf::from("project"), &Some(PathBuf::from("elsewhere")));
/// assert!(matches!(folder.to_str(), Some("elsewhere")));
/// ```
pub fn resolve_plugins(root: &std::path::Path, plugins: &Option<PathBuf>) -> PathBuf {
    plugins.clone().unwrap_or_else(|| root.join("plugins"))
}

/// Resolves one slot file path with `@` sugar.
///
/// Values starting with `@` strip the sigil and resolve under
/// the plans folder. Explicit paths pass through unchanged.
///
/// # Arguments
///
/// * `raw` - the output or input value under resolving.
///
/// # Returns
///
/// The named slot path for `@` values, else the input unchanged.
///
/// # Errors
///
/// Empty names, separator carriers, and dot segments fail
/// as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::cli::resolve_slot_file;
/// use std::path::Path;
///
/// assert!(matches!(resolve_slot_file(Path::new("slot.json")), Ok(path) if path == Path::new("slot.json")));
/// assert!(matches!(
///     resolve_slot_file(Path::new("@work")),
///     Ok(path) if path.ends_with("confit/plans/work.json")
/// ));
/// ```
pub fn resolve_slot_file(raw: &std::path::Path) -> Result<PathBuf, confit_core::error::Error> {
    let Some(text) = raw.to_str() else {
        return Ok(raw.to_path_buf());
    };
    let Some(name) = text.strip_prefix('@') else {
        return Ok(raw.to_path_buf());
    };
    confit_core::store::slots::resolve_named_slot(name)
}
