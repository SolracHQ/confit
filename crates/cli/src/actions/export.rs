//! Export run
//! State picker into bundle files or manifest prints.

use std::path::PathBuf;

use confit_core::error::Terminal;
use confit_core::error::{Error, ErrorCause, Failure, Outcome};
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::stored;
use confit_store::Stores;
use confit_store::state::Picker;

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
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ExportSlot`] for malformed slots.
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] for bundle writes.
    pub fn run(args: &'a ExportArgs, stores: Stores, sinks: Sinks) -> Outcome<ExportReport> {
        Self {
            args,
            stores,
            sinks,
        }
        .execute()
    }

    /// Resolve the picker, then write the bundle file or render manifest JSON.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::ExportSlot`] for malformed slots.
    /// - [`confit_core::error::kind::ErrorKind::ExportState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] for bundle writes.
    pub fn execute(self) -> Outcome<ExportReport> {
        let args = self.args;
        let stores = self.stores;
        let bundles = Bundler::new(&stores);
        let picker = match args.picker.as_deref() {
            None => Picker::Applied,
            Some(raw) => match raw.parse::<Picker>() {
                Ok(picker) => picker,
                Err(error) => match error.root_cause() {
                    ErrorCause::SlotReason(reason) => {
                        return Err(Error::export_slot(raw.to_owned(), reason.clone()));
                    }
                    _ => return Err(Error::export_state(error)),
                },
            },
        };
        let manifest = match stored::resolve(&stores.states(), &picker) {
            Ok(found) => found,
            Err(Failure::Terminal(terminal)) => return Err(terminal),
            Err(Failure::Mid(error)) => match error.root_cause() {
                ErrorCause::SlotReason(reason) => {
                    let raw = match &picker {
                        Picker::Applied => "applied".to_owned(),
                        Picker::Named(name) => format!("@{name}"),
                        Picker::History(pick) => format!("%{pick}"),
                    };
                    return Err(Error::export_slot(raw, reason.clone()));
                }
                _ => return Err(Error::export_state(error)),
            },
        };
        let stem = match &picker {
            Picker::Applied => APPLIED_STEM.to_owned(),
            Picker::Named(name) => name.clone(),
            Picker::History(pick) => format!("{HISTORY_STEM_PREFIX}{pick}"),
        };
        let auto = PathBuf::from(stem);
        if args.manifest {
            let text = manifest.json().write_bundle(&auto)?;
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
