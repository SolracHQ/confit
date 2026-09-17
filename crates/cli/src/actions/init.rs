//! Init run
//!
//! Project scaffolding from embedded resources.

use std::path::PathBuf;

use confit_core::error::{Error, Result};
use confit_core::fs::Filesystem;

use crate::cli::InitArgs;

/// Starter profile text written by init.
///
/// Ships from `resources/profile.lua` beside the crate. One shell
/// config holds one alias patch to config plus one eval patch
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
/// # Examples
///
/// ```text
/// use confit_cli::actions::init::InitReport;
/// use std::path::PathBuf;
///
/// let report = InitReport { profile: PathBuf::from("demo/profile.lua"), written: 13 };
/// assert!(matches!(report.written, 14));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitReport {
    /// Holds the scaffolded profile path.
    pub profile: PathBuf,
    /// Counts files written, profile plus stubs.
    pub written: usize,
}

/// One init run from flags on an injected backend.
///
/// # Examples
///
/// ```text
/// use confit_cli::actions::init::InitRunner;
/// use confit_cli::cli::InitArgs;
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use std::path::{Path, PathBuf};
///
/// let fs = MemoryFs::new();
/// let args = InitArgs { dir: PathBuf::from("demo") };
/// let runner = InitRunner { args: &args, fs: &fs };
/// assert!(matches!(runner.execute(), Ok(_)));
/// assert!(fs.exists(Path::new("demo/profile.lua")));
/// assert!(fs.exists(Path::new("demo/stubs/confit.d.lua")));
/// ```
pub struct InitRunner<'a> {
    /// Holds the init flags under running.
    pub args: &'a InitArgs,
    /// Holds the backend under reading plus writing.
    pub fs: &'a dyn Filesystem,
}

impl InitRunner<'_> {
    /// Writes one profile holding one rc document plus editor
    /// stubs copied from the binary. Present profile or stubs abort
    /// with nothing written.
    ///
    /// # Returns
    ///
    /// The scaffolded profile path plus the written file count.
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
        if self.fs.exists(&stubs_dir) {
            return Err(Error::Plan(format!(
                "init: '{}' already exists, remove it or pick another target",
                stubs_dir.display()
            )));
        }
        for dest in &dests {
            if self.fs.exists(dest) {
                return Err(Error::Plan(format!(
                    "init: '{}' already exists, remove it or pick another target",
                    dest.display()
                )));
            }
        }
        self.fs
            .write(&profile, PROFILE_TEXT.as_bytes())
            .map_err(Error::from)?;
        for (rel, text) in STUB_FILES.iter().copied() {
            self.fs
                .write(&self.args.dir.join(rel), text.as_bytes())
                .map_err(Error::from)?;
        }
        Ok(InitReport {
            profile,
            written: dests.len(),
        })
    }
}
