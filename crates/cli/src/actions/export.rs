//! Export run
//!
//! Slot picker into bundle files or manifest prints.

use std::path::PathBuf;

use confit_model::error::{Error, Result};
use confit_model::manifest::Manifest;
use confit_store::Stores;
use confit_store::handles::TrustedHandle;
use confit_store::slot::SlotKind;
use confit_store::slot::SlotStore;

use crate::cli::ExportArgs;

use crate::seams::{Sinks, timed};

/// Auto-name stem for exports of the applied slot.
const APPLIED_STEM: &str = "applied";
/// Auto-name stem prefix for exports of history entries.
const HISTORY_STEM_PREFIX: &str = "prev-";

/// Outcome of one export run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// Holds the bundle path for file exports, `None` for manifest prints.
    pub dest: Option<PathBuf>,
    /// Holds pretty manifest JSON for manifest prints, `None` for file exports.
    pub manifest: Option<String>,
}

/// One export run from flags to a bundle file or manifest print.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::export::ExportRunner;
/// use confit_cli::cli::ExportArgs;
/// use confit_store::{StoreRoots, Stores};
///
/// let args = ExportArgs { picker: None, output: None, manifest: true };
/// let stores = Stores::new(StoreRoots::standard());
/// let report = ExportRunner { args: &args, stores, sinks: Default::default() }.execute();
/// assert!(matches!(report, Ok(_) | Err(_)));
/// ```
pub struct ExportRunner<'a> {
    /// Holds the export flags under running.
    pub args: &'a ExportArgs,
    /// Holds the write capabilities for the run.
    pub stores: Stores,
    /// Holds the output senders for the run.
    pub sinks: Sinks,
}

impl<'a> ExportRunner<'a> {
    /// Reads flags and runs the full export flow.
    pub fn run(args: &'a ExportArgs, stores: Stores, sinks: Sinks) -> Result<ExportReport> {
        Self {
            args,
            stores,
            sinks,
        }
        .execute()
    }

    /// Resolves the picker, then writes the bundle file or
    /// renders pretty manifest JSON.
    ///
    /// The destination rides `-o` as a literal file path and
    /// gains `.cb` unless present. Omitted destinations derive
    /// from the slot. Bundle output prints the path downstream,
    /// never streams bytes.
    ///
    /// # Returns
    ///
    /// The bundle path for file exports, else the manifest text.
    ///
    /// # Errors
    ///
    /// Picker, load, and write failures surface as plan or
    /// io errors. `-o` and `--manifest` together refuse.
    pub fn execute(self) -> Result<ExportReport> {
        let args = self.args;
        let stores = self.stores;
        let sinks = self.sinks;
        if args.manifest && args.output.is_some() {
            return Err(Error::Plan(
                "export: '-o' plus '--manifest' refuse together, pick one".to_string(),
            ));
        }
        let slots = stores.slots();
        let (manifest, auto) = timed("export load", || {
            resolve_slot_bundle(args.picker.as_deref(), &slots)
        })?;
        if args.manifest {
            let text = timed("export manifest", || {
                manifest
                    .json()
                    .map_err(|error| Error::Plan(error.to_string()))
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
                .write(&manifest, &dest, sinks.progress.as_ref())
                .map_err(|error| Error::Plan(error.to_string()))
        })?;
        Ok(ExportReport {
            dest: Some(written.canonical().to_path_buf()),
            manifest: None,
        })
    }
}

/// Resolves one picker to its live manifest and auto bundle name.
///
/// Slot errors carry the export command name.
///
/// # Arguments
///
/// * `picker` - the raw picker value under resolving.
/// * `slots` - the slot store under reading.
///
/// # Returns
///
/// The live manifest holding blob refs and the slot-derived
/// bundle destination carrying `.cb`.
///
/// # Errors
///
/// Absent slots, malformed, out-of-range picks and
/// load failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::export::resolve_slot_bundle;
/// use confit_store::slot::SlotStore;
/// use confit_store::StoreRoots;
///
/// let slots = SlotStore::new(&StoreRoots::standard());
/// let (manifest, dest) = resolve_slot_bundle(None, &slots).unwrap();
/// assert_eq!(dest.extension().and_then(|ext| ext.to_str()), Some("cb"));
/// ```
pub fn resolve_slot_bundle(picker: Option<&str>, slots: &SlotStore) -> Result<(Manifest, PathBuf)> {
    let (manifest, kind) = slots
        .resolve(picker)
        .map_err(|error| Error::Plan(error.to_string()))?;
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
