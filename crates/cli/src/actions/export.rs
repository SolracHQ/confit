//! Export run
//!
//! Slot picker into bundle files or manifest prints.

use std::path::PathBuf;

use confit_core::error::{Error, Result};
use confit_core::plan::Bundle;
use confit_core::store::SlotKind;
use confit_core::store::manifest::manifest_json;
use confit_store::slot::SlotStore;

use crate::cli::ExportArgs;

use crate::seams::{Seams, timed};

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

/// One export run from flags on injected seams.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::export::ExportRunner;
/// use confit_cli::seams::Seams;
/// use confit_cli::cli::ExportArgs;
/// use confit_core::fs::memory::MemoryFs;
/// use confit_core::probe::MemoryProbe;
/// use std::io::Cursor;
///
/// let args = ExportArgs { picker: None, output: None, manifest: true };
/// let fs = MemoryFs::new();
/// let probe = MemoryProbe::new();
/// let mut input = Cursor::new(String::new());
/// let report = ExportRunner::run(&args, Seams::memory(&fs, &probe, &mut input));
/// assert!(matches!(report, Ok(_) | Err(_)));
/// ```
pub struct ExportRunner<'a> {
    /// Holds the export flags under running.
    pub args: &'a ExportArgs,
    /// Holds the injected filesystem, output, and sink.
    pub seams: Seams<'a>,
}

impl<'a> ExportRunner<'a> {
    /// Reads flags and runs the full export flow on injected seams.
    pub fn run(args: &'a ExportArgs, seams: Seams<'a>) -> Result<ExportReport> {
        Self { args, seams }.execute()
    }

    /// Resolves the picker, then writes the bundle file or
    /// renders pretty manifest JSON through injected seams.
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
        let seams = self.seams;
        if args.manifest && args.output.is_some() {
            return Err(Error::Plan(
                "export: '-o' plus '--manifest' refuse together, pick one".to_string(),
            ));
        }
        let slots = seams.stores.slots();
        let (bundle, auto) = timed("export load", || {
            resolve_slot_bundle(args.picker.as_deref(), &*slots)
        })?;
        if args.manifest {
            let text = timed("export manifest", || manifest_json(&bundle.manifest))?;
            return Ok(ExportReport {
                dest: None,
                manifest: Some(text),
            });
        }
        let dest = match args.output.as_deref() {
            Some(raw) => raw.to_path_buf(),
            None => auto,
        };
        seams.emit_writing_manifest(bundle.manifest.documents.len());
        let dest = timed("export write", || {
            seams
                .stores
                .bundles()
                .write(&bundle, &dest, seams.progress.as_ref())
        })?;
        Ok(ExportReport {
            dest: Some(dest),
            manifest: None,
        })
    }
}

/// Resolves one picker to its live bundle and auto bundle name.
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
/// The live bundle holding blob handles, plus the slot-derived
/// bundle destination carrying `.cb`.
///
/// # Errors
///
/// Absent slots, malformed, out-of-range picks and
/// load failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::export::resolve_slot_bundle;
/// use confit_store::slot::memory::MemorySlotStore;
/// use confit_store::slot::SlotStore;
///
/// let slots = MemorySlotStore::new();
/// assert!(matches!(resolve_slot_bundle(None, &slots), Err(_)));
/// assert!(matches!(resolve_slot_bundle(Some("@missing"), &slots), Err(_)));
/// assert!(matches!(resolve_slot_bundle(Some("%1"), &slots), Err(_)));
/// ```
pub fn resolve_slot_bundle(
    picker: Option<&str>,
    slots: &dyn SlotStore,
) -> Result<(Bundle, PathBuf)> {
    let (bundle, kind) = slots.resolve(picker).map_err(|error| match error {
        Error::Plan(detail) => Error::Plan(format!("export: {detail}")),
        other => other,
    })?;
    let stem = match kind {
        SlotKind::Applied => APPLIED_STEM.to_string(),
        SlotKind::Named(name) => name,
        SlotKind::History(pick) => format!("{HISTORY_STEM_PREFIX}{pick}"),
    };
    Ok((bundle, auto_dest(&stem)))
}

/// Builds one slot-derived bundle destination stem.
fn auto_dest(stem: &str) -> PathBuf {
    std::path::Path::new(stem).to_path_buf()
}
