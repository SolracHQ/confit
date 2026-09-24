//! Plan run
//!
//! Profile flags into built bundles with drift.

use std::path::Path;

use confit_core::drift::{Drift, DriftOrder};
use confit_core::error::Result;
use confit_core::plan::Bundle;

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
}

/// One plan run from plan flags to a built bundle.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::plan::PlanRunner;
/// use confit_cli::seams::Seams;
/// use confit_cli::cli::{PlanArgs, SharedArgs};
/// use confit_core::fs::memory::MemoryFs;
/// use confit_core::probe::MemoryProbe;
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
/// let fs = MemoryFs::new();
/// let probe = MemoryProbe::new();
/// let mut input = Cursor::new(String::new());
/// let runner = PlanRunner { args: &args, seams: Seams::memory(&fs, &probe, &mut input) };
/// let outcome = runner.execute();
/// assert!(matches!(outcome, Ok(_) | Err(_)));
/// ```
pub struct PlanRunner<'a> {
    /// Holds the plan flags under running.
    pub args: &'a PlanArgs,
    /// Holds the injected filesystem, output, and sink.
    pub seams: Seams<'a>,
}

impl PlanRunner<'_> {
    /// Evaluates the engine, loads previous manifest, diffs drift,
    /// builds the core bundle, and writes the payload on demand
    /// through injected seams.
    ///
    /// # Returns
    ///
    /// The built bundle with its previous manifest and drift.
    ///
    /// # Errors
    ///
    /// Evaluation, plan, build, and write failures surface
    /// as plan or io errors.
    pub fn execute(self) -> Result<PlanOutcome> {
        let evaluation = evaluate_shared(
            &self.args.shared,
            &self.args.profile,
            self.seams.progress.clone(),
            &self.seams.stores,
        )?;
        let documents = evaluation.documents;
        let slots = self.seams.stores.slots();
        let workspace = self.seams.stores.workspace();
        let blobs = self.seams.stores.blobs();
        let first_run = slots.is_first_run();
        let previous = slots.load()?;

        self.seams.emit_hashing();
        let mut built = timed("hash", || Bundle::build(documents, evaluation.hooks))?;
        built.blobs = evaluation.blobs;
        log_processed(&built, &previous);

        let drifts = timed("drift", || {
            if first_run {
                confit_store::drift::drift(&built, &*workspace, &*blobs, DriftOrder::DiskFirst)
            } else {
                confit_store::drift::drift(
                    &previous,
                    &*workspace,
                    &*blobs,
                    DriftOrder::RecordedFirst,
                )
            }
        });
        if self.args.output.is_some() {
            self.seams
                .emit_writing_manifest(built.manifest.documents.len());
        }
        timed("write", || match self.args.output.as_deref() {
            Some(dest) if is_named_output(dest) => {
                let name = dest
                    .to_str()
                    .and_then(|text| text.strip_prefix('@'))
                    .unwrap_or("");
                slots.store_named(name, &built)
            }
            Some(dest) => self
                .seams
                .stores
                .bundles()
                .write(&built, dest, self.seams.progress.as_ref())
                .map(|_| ()),
            None => Ok(()),
        })?;
        Ok(PlanOutcome {
            built,
            previous,
            drift: drifts,
            first_run,
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

#[cfg(test)]
mod tests {
    use super::*;

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
