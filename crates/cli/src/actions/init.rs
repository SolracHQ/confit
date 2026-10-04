//! Init run
//! Project scaffolding from embedded resources.

use std::path::PathBuf;

use confit_driver as driver;

use crate::error::{CliError, Result};

use crate::cli::InitArgs;

/// Starter profile text written by init.
///
/// Ships from `resources/profile.lua` beside the crate. One shell config holds
/// one alias patch to config and one eval patch to final. Comments guide first
/// use.
const PROFILE_TEXT: &str = include_str!("../../resources/profile.lua");

/// Stub files shipped inside the binary, keyed by target-relative path.
///
/// Module stubs land in a stubs dir beside the profile. Plugin stubs mirror the
/// plugin load layout beside their plugin names.
const STUB_FILES: &[(&str, &str)] = &[
    (
        "stubs/confit.resource.d.lua",
        include_str!("../../../../stubs/confit.resource.d.lua"),
    ),
    (
        "stubs/confit.validate.d.lua",
        include_str!("../../../../stubs/confit.validate.d.lua"),
    ),
    (
        "stubs/confit.version.d.lua",
        include_str!("../../../../stubs/confit.version.d.lua"),
    ),
    (
        "plugins/solrachq/mise/plugin.d.lua",
        include_str!("../../../engine-old/plugins/solrachq/mise/plugin.d.lua"),
    ),
    (
        "plugins/solrachq/nerd_fonts/plugin.d.lua",
        include_str!("../../../engine-old/plugins/solrachq/nerd_fonts/plugin.d.lua"),
    ),
    (
        "plugins/solrachq/merge/plugin.d.lua",
        include_str!("../../../engine-old/plugins/solrachq/merge/plugin.d.lua"),
    ),
    (
        "plugins/solrachq/template/plugin.d.lua",
        include_str!("../../../engine-old/plugins/solrachq/template/plugin.d.lua"),
    ),
];

/// Outcome of one init run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// The scaffolded profile path.
    pub profile: PathBuf,
    /// Counts files written, profile, and stubs.
    pub written: usize,
}

/// One init run from flags on the host driver.
pub struct InitRunner<'a> {
    /// The init flags under running.
    pub args: &'a InitArgs,
}

impl InitRunner<'_> {
    /// Writes one profile holding one rc document and editor stubs copied from
    /// the binary.
    ///
    /// The report carries the scaffolded profile path and the written file
    /// count.
    ///
    /// - Present profile or stubs abort with nothing written.
    ///
    /// # Errors
    ///
    /// - [`CliError::Exists`] for present scaffolds.
    /// - [`CliError::Write`] with [`CliError::WriteUnknown`]
    ///   for scaffold writes.
    pub fn execute(self) -> Result<InitReport> {
        let profile = self.args.dir.join("profile.lua");
        let mut dests = vec![profile.clone()];
        dests.extend(STUB_FILES.iter().map(|entry| self.args.dir.join(entry.0)));
        let stubs_dir = self.args.dir.join("stubs");
        if driver::fs::exists(&stubs_dir) {
            return Err(CliError::Exists { path: stubs_dir });
        }
        for dest in &dests {
            if driver::fs::exists(dest) {
                return Err(CliError::Exists { path: dest.clone() });
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
/// # Errors
///
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for folder and file writes.
fn write_file(dest: &std::path::Path, text: &[u8]) -> Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        driver::fs::create_dir_all(parent)
            .map_err(|error| CliError::from_write_io(parent, error))?;
    }
    driver::fs::write(dest, text).map_err(|error| CliError::from_write_io(dest, error))
}
