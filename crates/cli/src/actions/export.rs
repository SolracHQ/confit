//! Export run
//!
//! Slot picker into bundle files or manifest prints.

use std::path::PathBuf;

use confit_core::error::{Error, Result};
use confit_core::fs::Filesystem;
use confit_core::plan::Bundle;
use confit_core::store::bundle::write_bundle;
use confit_core::store::manifest::manifest_json;
use confit_core::store::slots::{SlotKind, resolve_slot as resolve_slot_plan};

use crate::actions::plan::ensure_bundle_extension;
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
/// use confit_cli::fs::OsFs;
/// use std::io::Cursor;
///
/// let args = ExportArgs { picker: None, output: None, manifest: true };
/// let fs = OsFs;
/// let mut input = Cursor::new(String::new());
/// let report = ExportRunner::run(&args, Seams::memory(&fs, &mut input));
/// assert!(matches!(report, Ok(_) | Err(_)));
/// ```
pub struct ExportRunner<'a> {
    /// Holds the export flags under running.
    pub args: &'a ExportArgs,
    /// Holds the injected filesystem plus output plus sink.
    pub seams: Seams<'a>,
}

impl<'a> ExportRunner<'a> {
    /// Reads flags plus runs the full export flow on injected seams.
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
    /// Picker plus load plus write failures surface as plan or
    /// io errors. `-o` plus `--manifest` together refuse.
    pub fn execute(self) -> Result<ExportReport> {
        let args = self.args;
        let seams = self.seams;
        if args.manifest && args.output.is_some() {
            return Err(Error::Plan(
                "export: '-o' plus '--manifest' refuse together, pick one".to_string(),
            ));
        }
        let fs: &dyn Filesystem = seams.fs;
        let (plan, auto) = timed("export load", || resolve_slot(args.picker.as_deref(), fs))?;
        if args.manifest {
            let text = timed("export manifest", || manifest_json(&plan))?;
            return Ok(ExportReport {
                dest: None,
                manifest: Some(text),
            });
        }
        let dest = match args.output.as_deref() {
            Some(raw) => ensure_bundle_extension(raw),
            None => auto,
        };
        seams.emit_writing_plan(plan.manifest.documents.len());
        timed("export write", || {
            write_bundle(&plan, &dest, fs, seams.progress.as_ref())
        })?;
        Ok(ExportReport {
            dest: Some(dest),
            manifest: None,
        })
    }
}

/// Resolves one picker to its live plan plus auto bundle name.
///
/// Slot errors carry the export command name.
///
/// # Arguments
///
/// * `picker` - the raw picker value under resolving.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes, plus the slot-derived
/// bundle destination carrying `.cb`.
///
/// # Errors
///
/// Absent slots plus malformed plus out-of-range picks plus
/// load failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::export::resolve_slot;
/// use confit_core::fs::MemoryFs;
///
/// let fs = MemoryFs::new();
/// assert!(matches!(resolve_slot(None, &fs), Err(_)));
/// assert!(matches!(resolve_slot(Some("backup.cb"), &fs), Err(_)));
/// assert!(matches!(resolve_slot(Some("%1"), &fs), Err(_)));
/// ```
pub fn resolve_slot(picker: Option<&str>, fs: &dyn Filesystem) -> Result<(Bundle, PathBuf)> {
    let (plan, kind) = resolve_slot_plan(picker, fs).map_err(|error| match error {
        Error::Plan(detail) => Error::Plan(format!("export: {detail}")),
        other => other,
    })?;
    let stem = match kind {
        SlotKind::Applied => APPLIED_STEM.to_string(),
        SlotKind::Named(name) => name,
        SlotKind::History(pick) => format!("{HISTORY_STEM_PREFIX}{pick}"),
    };
    Ok((plan, auto_dest(&stem)))
}

/// Builds one slot-derived bundle destination carrying `.cb`.
fn auto_dest(stem: &str) -> PathBuf {
    ensure_bundle_extension(std::path::Path::new(stem))
}
