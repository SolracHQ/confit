//! Export run
//! Slot picker into bundle files or manifest prints.

use std::path::PathBuf;

use confit_model::manifest::Manifest;
use confit_store::Stores;
use confit_store::handles::TrustedHandle;
use confit_store::slot::SlotKind;
use confit_store::slot::SlotStore;

use crate::error::{CliError, Result};

use crate::cli::ExportArgs;

use crate::sinks::{Sinks, timed};

/// Auto-name stem for exports of the applied slot.
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
    /// Runs the full export flow from flags.
    pub fn run(args: &'a ExportArgs, stores: Stores, sinks: Sinks) -> Result<ExportReport> {
        Self {
            args,
            stores,
            sinks,
        }
        .execute()
    }

    /// Resolves the picker, then writes the bundle file or renders pretty
    /// manifest JSON.
    ///
    /// The report carries the bundle path for file exports, else the
    /// manifest text.
    ///
    /// - The `-o` destination reads as a literal file path.
    /// - Omitted destinations derive from the slot.
    ///
    /// # Errors
    ///
    /// - [`CliError::FlagRefuse`] for joint flags.
    /// - [`CliError::Slot`] for slot resolves.
    /// - [`CliError::Model`] for manifest renders.
    /// - [`CliError::Bundle`] for bundle writes.
    pub fn execute(self) -> Result<ExportReport> {
        let args = self.args;
        let stores = self.stores;
        let sinks = self.sinks;
        if args.manifest && args.output.is_some() {
            return Err(CliError::FlagRefuse);
        }
        let slots = stores.slots();
        let (manifest, auto) = timed("export load", || {
            resolve_slot_bundle(args.picker.as_deref(), &slots)
        })?;
        if args.manifest {
            let text = timed("export manifest", || {
                manifest.json().map_err(CliError::from)
            })?;
            return Ok(ExportReport {
                dest: None,
                manifest: Some(text),
            });
        }
        let dest = match args.output.as_deref() {
            Some(raw) => raw.to_path_buf(),
            None => auto,
        };
        sinks.emit_writing_manifest(manifest.documents.len());
        let written = timed("export write", || {
            stores
                .bundles()
                .write(&manifest, &dest)
                .map_err(CliError::from)
        })?;
        Ok(ExportReport {
            dest: Some(written.canonical().to_path_buf()),
            manifest: None,
        })
    }
}

/// Resolves one picker to its live manifest and auto bundle name.
///
/// # Errors
///
/// - [`CliError::Slot`] for slot resolves.
pub fn resolve_slot_bundle(picker: Option<&str>, slots: &SlotStore) -> Result<(Manifest, PathBuf)> {
    let (manifest, kind) = slots.resolve(picker)?;
    let stem = match kind {
        SlotKind::Applied => APPLIED_STEM.to_string(),
        SlotKind::Named(name) => name,
        SlotKind::History(pick) => format!("{HISTORY_STEM_PREFIX}{pick}"),
    };
    Ok((manifest, auto_dest(&stem)))
}

/// Builds one slot-derived bundle destination stem.
fn auto_dest(stem: &str) -> PathBuf {
    std::path::Path::new(stem).to_path_buf()
}
