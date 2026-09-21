//! Plan run
//!
//! Profile flags into built bundles with drift.

use std::path::{Path, PathBuf};

use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::Result;
use confit_core::fs::{
    Filesystem,
    snapshot::{snapshot_document, snapshot_tree},
};

use confit_core::document::ManifestDocument;
use confit_core::hook::lifecycle_lines;
use confit_core::ids::DocPath;
use confit_core::plan::Bundle;
use confit_core::store::bundle::write_bundle;
use confit_core::store::slots::{load_state, write_manifest};

use crate::cli::PlanArgs;

use crate::seams::{Seams, evaluate_shared, log_processed, timed};

/// Outcome of one profile run with its previous manifest.
#[derive(Debug)]
pub struct PlanOutcome {
    /// Holds the built bundle with counts.
    pub built: Bundle,
    /// Holds the previous manifest backing lifecycle marks.
    pub previous: Bundle,
    /// Holds disk edits leading the summary, desired versus
    /// disk on first runs.
    pub drift: Vec<Drift>,
    /// Holds true while the state slot file reads absent.
    pub first_run: bool,
    /// Holds hook lifecycle lines beside the summary.
    pub hook_lines: Vec<String>,
    /// Holds the tmp manifest path while no output destination passes.
    pub stored: Option<PathBuf>,
}

/// One plan run from plan flags to a built bundle.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::plan::PlanRunner;
/// use confit_cli::seams::Seams;
/// use confit_cli::cli::{PlanArgs, SharedArgs};
/// use confit_cli::fs::OsFs;
/// use std::io::Cursor;
/// use std::path::PathBuf;
///
/// let args = PlanArgs {
///     profile: PathBuf::from("profile.lua"),
///     shared: SharedArgs {
///         root: None,
///         plugins: None,
///         re_fetch: false,
///     },
///     output: None,
/// };
/// let fs = OsFs;
/// let mut input = Cursor::new(String::new());
/// let runner = PlanRunner { args: &args, store_tmp: false, seams: Seams::memory(&fs, &mut input) };
/// let outcome = runner.execute();
/// assert!(matches!(outcome, Ok(_) | Err(_)));
/// ```
pub struct PlanRunner<'a> {
    /// Holds the plan flags under running.
    pub args: &'a PlanArgs,
    /// Stores the payload under tmp while no destination passes.
    pub store_tmp: bool,
    /// Holds the injected filesystem plus output plus sink.
    pub seams: Seams<'a>,
}

impl PlanRunner<'_> {
    /// Evaluates the engine, loads previous manifest, diffs drift,
    /// builds the core bundle, and writes the payload on demand
    /// through injected seams.
    ///
    /// # Returns
    ///
    /// The built bundle with its previous manifest plus drift.
    ///
    /// # Errors
    ///
    /// Evaluation plus plan plus build plus write failures surface
    /// as plan or io errors.
    pub fn execute(self) -> Result<PlanOutcome> {
        let evaluation = evaluate_shared(
            &self.args.shared,
            &self.args.profile,
            self.seams.progress.clone(),
        )?;
        let documents = evaluation.documents;
        let state_file = confit_core::store::slots::default_state_path()?;
        self.seams.emit_reading_plan(&state_file);
        let fs: &dyn Filesystem = self.seams.fs;
        let first_run = !fs.exists(&state_file);
        let previous = load_state(Some(&state_file), fs)?;
        let snapshot = |document: &ManifestDocument| snapshot_document(document, fs);
        let snapshot_tree = |path: &DocPath| snapshot_tree(&path.expand(), fs);
        self.seams.emit_hashing();
        let mut built = timed("hash", || Bundle::build(documents, evaluation.hooks))?;
        built.blobs = evaluation.blobs;
        log_processed(&built, &previous);
        let order = if first_run {
            DriftOrder::DiskFirst
        } else {
            DriftOrder::RecordedFirst
        };
        let drifts = timed("drift", || {
            if first_run {
                built.drift(&snapshot, &snapshot_tree, order)
            } else {
                previous.drift(&snapshot, &snapshot_tree, order)
            }
        });
        let hook_lines = lifecycle_lines(&built.manifest.hooks, &previous.manifest.hooks);
        if self.args.output.is_some() || self.store_tmp {
            self.seams
                .emit_writing_manifest(built.manifest.documents.len());
        }
        let stored = timed("write", || {
            match (self.args.output.as_deref(), self.store_tmp) {
                (Some(dest), _) if is_named_output(dest) => {
                    let resolved = crate::cli::resolve_slot_file(dest)?;
                    write_manifest(&built, Some(&resolved), fs, self.seams.progress.as_ref())
                        .map(|()| None)
                }
                (Some(dest), _) => {
                    let resolved = crate::cli::resolve_slot_file(dest)?;
                    let dest = ensure_bundle_extension(&resolved);
                    write_bundle(&built, &dest, fs, self.seams.progress.as_ref()).map(|()| None)
                }
                (None, true) => {
                    let tmp = tmp_plan_path();
                    write_bundle(&built, &tmp, fs, self.seams.progress.as_ref()).map(|()| Some(tmp))
                }
                (None, false) => Ok(None),
            }
        })?;
        Ok(PlanOutcome {
            built,
            previous,
            drift: drifts,
            first_run,
            hook_lines,
            stored,
        })
    }
}

/// Reports true while one output value names a stored slot.
///
/// Values starting with `@` resolve under the plans folder
/// and keep slot semantics. Explicit paths write bundles.
///
/// # Arguments
///
/// * `raw` - the output value under checking.
///
/// # Returns
///
/// True for `@` values, else false.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::plan::is_named_output;
/// use std::path::Path;
///
/// assert!(matches!(is_named_output(Path::new("@work")), true));
/// assert!(matches!(is_named_output(Path::new("plan.cb")), false));
/// ```
pub fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}

/// Bundle file extension imposed on explicit plan outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Ensures one bundle destination carries the bundle extension.
///
/// Bare paths gain the suffix, so creators always emit
/// bundles. Slot outputs never pass here and keep their
/// own names.
///
/// # Arguments
///
/// * `dest` - the resolved bundle path under checking.
///
/// # Returns
///
/// The input while the suffix already lands, else the input
/// with the suffix appended.
pub fn ensure_bundle_extension(dest: &Path) -> PathBuf {
    if dest
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(BUNDLE_EXTENSION))
    {
        dest.to_path_buf()
    } else {
        let mut name = dest.as_os_str().to_owned();
        name.push(".cb");
        PathBuf::from(name)
    }
}

/// Builds one tmp plan path under the OS temp folder.
///
/// The process id separates parallel runs. Tmp cleanup stays
/// the OS job.
///
/// # Returns
///
/// The tmp destination for the plan payload.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::plan::tmp_plan_path;
///
/// let path = tmp_plan_path();
/// assert!(matches!(path.to_string_lossy().contains("confit-plan-"), true));
/// ```
pub fn tmp_plan_path() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let slot = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "confit-plan-{}-{slot}.{BUNDLE_EXTENSION}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_output_gains_cb_suffix() {
        let out = ensure_bundle_extension(Path::new("plan"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("bare output gains suffix"),
        }
    }

    #[test]
    fn cb_suffix_stays_unchanged() {
        let out = ensure_bundle_extension(Path::new("plan.cb"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("cb output stays unchanged"),
        }
    }

    #[test]
    fn cb_suffix_match_reads_case_insensitive() {
        let out = ensure_bundle_extension(Path::new("plan.CB"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.CB"),
            None => panic!("uppercase cb stays untouched"),
        }
    }

    #[test]
    fn named_output_stays_exempt_from_bundle_suffix() {
        assert!(
            is_named_output(Path::new("@work")),
            "@name reads as slot output"
        );
        assert!(
            !is_named_output(Path::new("plan.cb")),
            "explicit bundle reads as file output"
        );
        assert!(
            !is_named_output(Path::new("plan")),
            "bare path reads as file output"
        );
        assert!(
            !is_named_output(Path::new("plan.CB")),
            "uppercase bundle reads as file output"
        );
    }
}
