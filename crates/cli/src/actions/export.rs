//! Export run
//! State picker into bundle files or manifest prints.

use std::path::PathBuf;

use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::Manifest;
use confit_runtime::manifest::stored;
use confit_store::Stores;
use confit_store::state::Picker;
use confit_store::state::StateStore;

use crate::error::{CliError, Result};

use crate::cli::ExportArgs;

use crate::sinks::Sinks;

/// Auto-name stem for exports of the applied state.
const APPLIED_STEM: &str = "applied";
/// Auto-name stem prefix for exports of history entries.
const HISTORY_STEM_PREFIX: &str = "prev-";

/// Outcome of one export run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// The bundle path for file exports, `None` for manifest prints.
    pub dest: Option<PathBuf>,
    /// Pretty manifest JSON for manifest prints, `None` for file exports.
    pub manifest: Option<String>,
}

/// One export run from flags to a bundle file or manifest print.
pub struct ExportRunner<'a> {
    /// The export flags under running.
    pub args: &'a ExportArgs,
    /// The write capabilities for the run.
    pub stores: Stores,
    /// The output senders for the run.
    pub sinks: Sinks,
}

impl<'a> ExportRunner<'a> {
    /// Run the full export flow from flags.
    pub fn run(args: &'a ExportArgs, stores: Stores, sinks: Sinks) -> Result<ExportReport> {
        Self {
            args,
            stores,
            sinks,
        }
        .execute()
    }

    /// Resolve the picker, then write the bundle file or render pretty
    /// manifest JSON.
    ///
    /// Report with the bundle path for file exports, else the
    /// manifest text.
    ///
    /// - The `-o` destination is a literal file path.
    /// - Omitted destinations derive from the state.
    ///
    /// # Errors
    ///
    /// - [`CliError::FlagRefuse`] for joint flags.
    /// - [`CliError::BadPicker`] for malformed pickers.
    /// - [`CliError::Runtime`] for state resolves and bundle writes.
    /// - [`CliError::Model`] for manifest renders.
    pub fn execute(self) -> Result<ExportReport> {
        let args = self.args;
        let stores = self.stores;
        if args.manifest && args.output.is_some() {
            return Err(CliError::FlagRefuse);
        }
        let bundles = Bundler::new(&stores);
        let picker = match args.picker.as_deref() {
            None => Picker::Applied,
            Some(raw) => raw.parse().map_err(|_| CliError::BadPicker {
                input: raw.to_owned(),
            })?,
        };
        let (manifest, auto) = resolve_state_bundle(&picker, &stores.states())?;
        if args.manifest {
            let text = manifest.json().map_err(CliError::from)?;
            return Ok(ExportReport {
                dest: None,
                manifest: Some(text),
            });
        }
        let dest = match args.output.as_deref() {
            Some(raw) => raw.to_path_buf(),
            None => auto,
        };
        let dest = bundles.write(&manifest, &dest).map_err(CliError::from)?;
        Ok(ExportReport {
            dest: Some(dest),
            manifest: None,
        })
    }
}

/// Resolve one picker to its stored manifest and auto bundle name.
///
/// # Errors
///
/// - [`CliError::Runtime`] for state resolves.
pub fn resolve_state_bundle(picker: &Picker, states: &StateStore) -> Result<(Manifest, PathBuf)> {
    let manifest = stored::resolve(states, picker)?;
    let stem = match picker {
        Picker::Applied => APPLIED_STEM.to_string(),
        Picker::Named(name) => name.clone(),
        Picker::History(pick) => format!("{HISTORY_STEM_PREFIX}{pick}"),
    };
    Ok((manifest, auto_dest(&stem)))
}

/// Build one state-derived bundle destination stem.
fn auto_dest(stem: &str) -> PathBuf {
    std::path::Path::new(stem).to_path_buf()
}
