//! Init run
//!
//! Project scaffolding from embedded resources.

use std::path::PathBuf;

use confit_driver as driver;
use confit_model::error::{Error, Result};

use crate::cli::InitArgs;

/// Starter profile text written by init.
///
/// Ships from `resources/profile.lua` beside the crate. One shell
/// config holds one alias patch to config and one eval patch
/// to final. Comments guide first use.
const PROFILE_TEXT: &str = include_str!("../../resources/profile.lua");

/// Stub files shipped inside the binary, keyed by target-relative path.
///
/// Namespace stubs land in a stubs dir beside the profile. Plugin
/// stubs mirror the plugin load layout beside their plugin names.
const STUB_FILES: &[(&str, &str)] = &[
    (
        "stubs/confit.d.lua",
        include_str!("../../../../stubs/confit.d.lua"),
    ),
    (
        "stubs/namespaces/config.d.lua",
        include_str!("../../../../stubs/namespaces/config.d.lua"),
    ),
    (
        "stubs/namespaces/document.d.lua",
        include_str!("../../../../stubs/namespaces/document.d.lua"),
    ),
    (
        "stubs/namespaces/hook.d.lua",
        include_str!("../../../../stubs/namespaces/hook.d.lua"),
    ),
    (
        "stubs/namespaces/patch.d.lua",
        include_str!("../../../../stubs/namespaces/patch.d.lua"),
    ),
    (
        "stubs/namespaces/paths.d.lua",
        include_str!("../../../../stubs/namespaces/paths.d.lua"),
    ),
    (
        "stubs/namespaces/plugin.d.lua",
        include_str!("../../../../stubs/namespaces/plugin.d.lua"),
    ),
    (
        "stubs/namespaces/resources.d.lua",
        include_str!("../../../../stubs/namespaces/resources.d.lua"),
    ),
    (
        "stubs/namespaces/runtime.d.lua",
        include_str!("../../../../stubs/namespaces/runtime.d.lua"),
    ),
    (
        "stubs/namespaces/utils.d.lua",
        include_str!("../../../../stubs/namespaces/utils.d.lua"),
    ),
    (
        "plugins/solrachq/mise/plugin.d.lua",
        include_str!("../../../engine/plugins/solrachq/mise/plugin.d.lua"),
    ),
    (
        "plugins/solrachq/nerd_fonts/plugin.d.lua",
        include_str!("../../../engine/plugins/solrachq/nerd_fonts/plugin.d.lua"),
    ),
    (
        "plugins/solrachq/merge/plugin.d.lua",
        include_str!("../../../engine/plugins/solrachq/merge/plugin.d.lua"),
    ),
    (
        "plugins/solrachq/template/plugin.d.lua",
        include_str!("../../../engine/plugins/solrachq/template/plugin.d.lua"),
    ),
];

/// Outcome of one init run.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Holds the scaffolded profile path.
    pub profile: PathBuf,
    /// Counts files written, profile, and stubs.
    pub written: usize,
}

/// One init run from flags on the host driver.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::init::InitRunner;
/// use confit_cli::cli::InitArgs;
/// use std::path::PathBuf;
///
/// let args = InitArgs { dir: PathBuf::from("demo") };
/// let report = InitRunner { args: &args }.execute().unwrap();
/// assert_eq!(report.profile, PathBuf::from("demo/profile.lua"));
/// ```
pub struct InitRunner<'a> {
    /// Holds the init flags under running.
    pub args: &'a InitArgs,
}

impl InitRunner<'_> {
    /// Writes one profile holding one rc document and editor
    /// stubs copied from the binary. Present profile or stubs abort
    /// with nothing written.
    ///
    /// # Returns
    ///
    /// The scaffolded profile path and the written file count.
    ///
    /// # Errors
    ///
    /// Present profile or stubs fail as plan errors. Write
    /// failures surface as io errors.
    pub fn execute(self) -> Result<InitReport> {
        let profile = self.args.dir.join("profile.lua");
        let mut dests = vec![profile.clone()];
        dests.extend(STUB_FILES.iter().map(|entry| self.args.dir.join(entry.0)));
        let stubs_dir = self.args.dir.join("stubs");
        if driver::exists(&stubs_dir) {
            return Err(Error::Plan(format!(
                "init: '{}' already exists, remove it or pick another target",
                stubs_dir.display()
            )));
        }
        for dest in &dests {
            if driver::exists(dest) {
                return Err(Error::Plan(format!(
                    "init: '{}' already exists, remove it or pick another target",
                    dest.display()
                )));
            }
        }
        write_file(&profile, PROFILE_TEXT.as_bytes())?;
        for (rel, text) in STUB_FILES.iter().copied() {
            write_file(&self.args.dir.join(rel), text.as_bytes())?;
        }
        Ok(InitReport {
            profile,
            written: dests.len(),
        })
    }
}

/// Writes one scaffold file creating parent folders first.
///
/// The old seam created parents inside `write`; the driver
/// does not, so init names the step explicitly.
///
/// # Errors
///
/// Unwritable folders and files fail as io errors.
fn write_file(dest: &std::path::Path, text: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        driver::create_dir_all(parent).map_err(Error::from)?;
    }
    driver::write(dest, text).map_err(Error::from)
}
