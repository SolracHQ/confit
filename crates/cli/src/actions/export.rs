//! Packs one state into a bundle file or prints its manifest.

use std::path::PathBuf;

use confit_bundler::Bundler;
use confit_core::error::{ErrorCause, TerminalResult, error, kind::ErrorKind};
use confit_events::EventHandles;
use confit_store::{Stores, state::Picker};

use crate::actions::ArtifactReport;
use crate::cli::ExportArgs;

/// The auto-name prefix for exports of the applied state.
const APPLIED_PREFIX: &str = "applied";
/// The auto-name prefix for exports of history entries.
const HISTORY_PREFIX: &str = "prev-";

pub struct ExportRunner<'a> {
    /// The flags that pick the state and the output.
    pub args: &'a ExportArgs,
    /// The trusted roots and content caches.
    pub stores: Stores,
}

impl<'a> ExportRunner<'a> {
    /// Runs the export flow and returns the report.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] when the state picker text is malformed.
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] when a state fails.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the manifest version is unsupported.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when a stored manifest has no version.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] when the bundle write fails.
    pub fn run(args: &'a ExportArgs, stores: Stores) -> TerminalResult<ArtifactReport> {
        Self { args, stores }.execute()
    }

    /// Resolves the picker, then writes the file or returns the manifest.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] when the state picker text is malformed.
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] when a state fails.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the manifest version is unsupported.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when a stored manifest has no version.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] when the bundle write fails.
    pub fn execute(self) -> TerminalResult<ArtifactReport> {
        let args = self.args;
        let stores = self.stores;
        let bundles = Bundler::new(&stores, EventHandles::silent());
        let states = stores.states();
        let (manifest, prefix) = match args.picker.as_deref() {
            None => (states.load_applied()?, APPLIED_PREFIX.to_owned()),
            Some(raw) => match raw.parse::<Picker>() {
                Ok(picker) => {
                    let prefix = match &picker {
                        Picker::Named(name) => name.clone(),
                        Picker::History(pick) => format!("{HISTORY_PREFIX}{pick}"),
                    };
                    (states.load_state(&picker)?, prefix)
                }
                Err(contextual) => match contextual.cause() {
                    ErrorCause::StateReason(_) => {
                        return Err(contextual
                            .with_state(raw.to_owned())
                            .terminal(ErrorKind::ExportState));
                    }
                    _ => return error(ErrorKind::ExportState, contextual),
                },
            },
        };
        if args.manifest {
            let source = args.picker.as_deref().unwrap_or(APPLIED_PREFIX).to_owned();
            return Ok(ArtifactReport {
                output: source,
                manifest,
            });
        }
        let auto = PathBuf::from(prefix);
        let dest = match args.output.as_deref() {
            Some(raw) => raw.to_path_buf(),
            None => auto,
        };
        let dest = bundles.write(&manifest, &dest)?;
        Ok(ArtifactReport {
            output: dest.display().to_string(),
            manifest,
        })
    }
}
