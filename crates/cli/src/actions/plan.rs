//! Plan run
//! Profile flags into built bundles with drift.

use std::path::Path;

use confit_driver as driver;
use confit_store::Stores;

use crate::error::{CliError, Result};

use crate::cli::{PlanArgs, plan_profile};
use crate::presentation::summary::render_prepared;

use crate::sinks::Sinks;

/// Successful plan run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanReport {
    /// Rendered summary text for display.
    pub text: String,
}

/// Plan run from plan flags to a built bundle.
pub struct PlanRunner<'a> {
    /// Plan flags for the run.
    pub args: &'a PlanArgs,
    /// Write capabilities for the run.
    pub stores: Stores,
    /// Output senders for the run.
    pub sinks: Sinks,
}

impl PlanRunner<'_> {
    /// Prepare the shared preview and write the payload on demand.
    ///
    /// Report with the rendered preview.
    ///
    /// # Errors
    ///
    /// - [`crate::error::CliError::Slot`] for slot reads and writes.
    /// - [`crate::error::CliError::Bundle`] for bundle writes.
    /// - [`crate::error::CliError::Engine`] for evaluation failures.
    /// - [`crate::error::CliError::NoDefaultProfile`] for absent defaults.
    /// - [`crate::error::CliError::NoProfile`] for absent profiles.
    pub fn execute(self) -> Result<PlanReport> {
        let path = plan_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && driver::fs::is_missing(&path) {
            return Err(CliError::NoDefaultProfile {
                project: self.args.project.clone(),
            });
        }
        let desired = confit_engine::Engine::new(self.stores.clone()).evaluate(&path)?;
        let prepared = confit_runtime::prepare(&self.stores, desired)?;
        let text = render_prepared(&prepared);
        if let Some(output) = self.args.output.as_deref() {
            if is_named_output(output) {
                let name = output
                    .to_str()
                    .and_then(|text| text.strip_prefix('@'))
                    .unwrap_or("");
                self.stores
                    .slots()
                    .store_named(name, &prepared.desired)
                    .map_err(CliError::from)?;
            } else {
                self.stores
                    .bundles()
                    .write(&prepared.desired, output)
                    .map(|_| ())
                    .map_err(CliError::from)?;
            }
        }
        Ok(PlanReport { text })
    }
}

/// Report whether one output value shows a stored slot.
fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}
