//! Plan run
//! Profile flags into built bundles with drift.

use std::path::Path;

use confit_model::drift::{Drift, DriftOrder};
use confit_model::manifest::Manifest;
use confit_runtime::Applier;
use confit_store::Stores;

use crate::error::{CliError, Result};

use crate::cli::PlanArgs;

use crate::seams::{Sinks, evaluate_shared, log_processed, timed};

/// Outcome of one profile run with its previous manifest.
#[derive(Debug)]
pub struct PlanOutcome {
    /// The built manifest with counts.
    pub built: Manifest,
    /// The previous manifest backing lifecycle marks.
    pub previous: Manifest,
    /// Holds disk edits leading the summary, desired versus disk on first runs.
    pub drift: Vec<Drift>,
    /// True while the state slot file reads absent.
    pub first_run: bool,
}

/// One plan run from plan flags to a built bundle.
pub struct PlanRunner<'a> {
    /// The plan flags under running.
    pub args: &'a PlanArgs,
    /// The write capabilities for the run.
    pub stores: Stores,
    /// The destination reads and writes for the run.
    pub applier: Applier,
    /// The output senders for the run.
    pub sinks: Sinks,
}

impl PlanRunner<'_> {
    /// Evaluates the engine, loads previous manifest, diffs drift, builds the
    /// core bundle, and writes the payload on demand.
    ///
    /// The outcome carries the built bundle with its previous manifest and
    /// drift.
    ///
    /// # Errors
    ///
    /// - [`crate::error::CliError::Slot`] for slot reads and writes.
    /// - [`crate::error::CliError::Bundle`] for bundle writes.
    /// - [`crate::error::CliError::Model`] for manifest builds.
    /// - [`crate::error::CliError::Engine`] for evaluation failures.
    pub fn execute(self) -> Result<PlanOutcome> {
        let evaluation = evaluate_shared(
            &self.args.shared,
            &self.args.profile,
            Some(self.sinks.progress.clone()),
            &self.stores,
        )?;
        let documents = evaluation.documents;
        let slots = self.stores.slots();
        let first_run = slots.is_first_run();
        let previous = slots.load()?;

        self.sinks.emit_hashing();
        let built = timed("hash", || Manifest::build(documents, evaluation.hooks))?;
        log_processed(&built, &previous);

        let drifts = timed("drift", || {
            if first_run {
                self.applier.drift(&built, DriftOrder::DiskFirst)
            } else {
                self.applier.drift(&previous, DriftOrder::RecordedFirst)
            }
        });
        if self.args.output.is_some() {
            self.sinks.emit_writing_manifest(built.documents.len());
        }
        timed("write", || match self.args.output.as_deref() {
            Some(dest) if is_named_output(dest) => {
                let name = dest
                    .to_str()
                    .and_then(|text| text.strip_prefix('@'))
                    .unwrap_or("");
                slots.store_named(name, &built).map_err(CliError::from)
            }
            Some(dest) => self
                .stores
                .bundles()
                .write(&built, dest)
                .map(|_| ())
                .map_err(CliError::from),
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
pub fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_output_stays_exempt_from_cb_suffix() {
        assert!(
            is_named_output(Path::new("@work")),
            "@name reads as slot output"
        );
        assert!(
            !is_named_output(Path::new("plan.cb")),
            "explicit path reads as file output"
        );
        assert!(
            !is_named_output(Path::new("plan")),
            "bare path reads as file output"
        );
        assert!(
            !is_named_output(Path::new("plan.CB")),
            "uppercase suffix reads as file output"
        );
    }
}
