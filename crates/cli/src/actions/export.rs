//! Packs one state into a bundle file or prints its manifest.

use std::path::PathBuf;

use confit_core::error::{
    ErrorCause, Terminal, TerminalResult, WithContext, error, kind::ErrorKind,
};
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::stored;
use confit_store::Stores;
use confit_store::state::Picker;

use crate::cli::ExportArgs;

/// The auto-name prefix for exports of the applied state.
const APPLIED_PREFIX: &str = "applied";
/// The auto-name prefix for exports of history entries.
const HISTORY_PREFIX: &str = "prev-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// The bundle path for a file export, `None` for a manifest print.
    pub dest: Option<PathBuf>,
    /// The pretty manifest JSON for a manifest print, `None` for a file export.
    pub manifest: Option<String>,
}

pub struct ExportRunner<'a> {
    /// The flags that pick the state and the output.
    pub args: &'a ExportArgs,
    /// The stores that hold the state and the resources.
    pub stores: Stores,
}

impl<'a> ExportRunner<'a> {
    /// Runs the export flow and returns the report.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ExportSlot`] when a slot is malformed.
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] when a state fails.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the manifest version is unsupported.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when a stored manifest has no version.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] when the bundle write fails.
    pub fn run(args: &'a ExportArgs, stores: Stores) -> TerminalResult<ExportReport> {
        Self { args, stores }.execute()
    }

    /// Resolves the picker, then writes the file or renders the manifest.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ExportSlot`] when a slot is malformed.
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] when a state fails.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the manifest version is unsupported.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when a stored manifest has no version.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] when the bundle write fails.
    pub fn execute(self) -> TerminalResult<ExportReport> {
        let args = self.args;
        let stores = self.stores;
        let bundles = Bundler::new(&stores);
        let picker = match args.picker.as_deref() {
            None => Picker::Applied,
            Some(raw) => match raw.parse::<Picker>() {
                Ok(picker) => picker,
                Err(contextual) => match contextual.cause() {
                    ErrorCause::SlotReason(_) => {
                        return Err(contextual
                            .with_slot(raw.to_owned())
                            .terminal(ErrorKind::ExportSlot));
                    }
                    _ => return error(ErrorKind::ExportState, contextual),
                },
            },
        };
        let manifest = stored::resolve(&stores.states(), &picker)?;
        let prefix = match &picker {
            Picker::Applied => APPLIED_PREFIX.to_owned(),
            Picker::Named(name) => name.clone(),
            Picker::History(pick) => format!("{HISTORY_PREFIX}{pick}"),
        };
        let auto = PathBuf::from(prefix);
        if args.manifest {
            let text = manifest
                .json()
                .with_bundle_path(&auto)
                .terminal(ErrorKind::WriteBundle)?;
            return Ok(ExportReport {
                dest: None,
                manifest: Some(text),
            });
        }
        let dest = match args.output.as_deref() {
            Some(raw) => raw.to_path_buf(),
            None => auto,
        };
        let dest = bundles.write(&manifest, &dest)?;
        Ok(ExportReport {
            dest: Some(dest),
            manifest: None,
        })
    }
}
