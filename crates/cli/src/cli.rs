//! Cli
//!
//! Argument shapes for the terminal surface.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// CaC: configuration as code for one user, with plan-before-apply.
///
/// # Examples
///
/// ```text
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
/// ```text
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "plan", "p.lua"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Plan(_))));
/// ```
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Evaluate the profile, diff against previous state, write the plan.
    Plan(PlanArgs),
    /// Preview the plan, confirm, and create every document in place.
    Apply(ApplyArgs),
    /// List stored plans, re-apply the picked one by index.
    Recover(RecoverArgs),
    /// Scaffold one profile plus stubs in the target folder.
    Init(InitArgs),
}

/// Shared run flags carried by plan plus apply.
///
/// The profile rides positionally on each command instead,
/// required by `plan`, required unless `--plan` on `apply`.
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
/// ```text
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
    /// Plan destination. Omitted stores the payload under tmp and prints the path.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Arguments for `confit apply`.
///
/// # Examples
///
/// ```text
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "apply", "p.lua", "--force"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Apply(_))));
/// ```
#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// Profile Lua file under evaluation. Omitted while `--plan` passes.
    #[arg(required_unless_present = "plan")]
    pub profile: Option<PathBuf>,
    /// Shared seam flags.
    #[command(flatten)]
    pub shared: SharedArgs,
    /// Reviewed plan file. Runs on the file alone with no profile.
    #[arg(long)]
    pub plan: Option<PathBuf>,
    /// Skips the confirmation prompt. Drift still re-prompts.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `confit recover`.
///
/// # Examples
///
/// ```text
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "recover"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Recover(_))));
/// ```
#[derive(Debug, Args)]
pub struct RecoverArgs {
    /// Stored plan index from the listing. Omitted lists only.
    pub index: Option<usize>,
    /// Skips the confirmation prompt. Drift still re-prompts.
    #[arg(long)]
    pub force: bool,
}

/// Arguments for `confit init`.
///
/// # Examples
///
/// ```text
/// use confit_cli::cli::{Cli, Command};
/// use clap::Parser;
///
/// let cli = Cli::try_parse_from(["confit", "init", "demo"]);
/// assert!(matches!(cli.map(|parsed| parsed.command), Ok(Command::Init(_))));
/// ```
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Target folder gaining the profile plus stubs. Omitted means the current folder.
    #[arg(default_value = ".")]
    pub dir: PathBuf,
}

/// Expands one leading `~` against the OS home folder.
///
/// Bare `~` plus `~/` prefixes resolve, everything else passes
/// through untouched. Missing home folders pass through too,
/// letting the caller fail with its own context.
///
/// # Arguments
///
/// * `path` - the raw arg path.
///
/// # Returns
///
/// The home-joined path, else the input unchanged.
///
/// # Examples
///
/// ```text
/// use confit_cli::cli::expand_tilde;
/// use std::path::Path;
///
/// assert!(matches!(expand_tilde(Path::new("rel/x")).to_str(), Some("rel/x")));
/// ```
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
            opt(&mut args.profile);
            shared(&mut args.shared);
            opt(&mut args.plan);
        }
        Command::Recover(_) => {}
        Command::Init(args) => {
            args.dir = expand_tilde(&args.dir);
        }
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
/// ```text
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
/// ```text
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

/// Resolves one plan file path with `@` sugar.
///
/// Values starting with `@` strip the sigil and resolve under
/// the plans folder. Explicit paths pass through unchanged.
///
/// # Arguments
///
/// * `raw` - the output or plan value under resolving.
///
/// # Returns
///
/// The named plan path for `@` values, else the input unchanged.
///
/// # Errors
///
/// Empty names plus separator carriers plus dot segments fail
/// as plan errors.
///
/// # Examples
///
/// ```text
/// use confit_cli::cli::resolve_plan_file;
/// use std::path::Path;
///
/// assert!(matches!(resolve_plan_file(Path::new("plan.json")), Ok(_)));
/// assert!(matches!(resolve_plan_file(Path::new("@work")), Ok(_)));
/// ```
pub fn resolve_plan_file(raw: &std::path::Path) -> Result<PathBuf, confit_core::error::Error> {
    let Some(text) = raw.to_str() else {
        return Ok(raw.to_path_buf());
    };
    let Some(name) = text.strip_prefix('@') else {
        return Ok(raw.to_path_buf());
    };
    confit_core::store::resolve_named_plan(name)
}
