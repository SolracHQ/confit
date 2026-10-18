//! Plan run
//! Profile flags into built bundles with differences.

use std::path::Path;

use confit_engine::Engine;
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::Manifest;
use confit_runtime::prepare;
use confit_store::Stores;
use confit_store::state::Picker;

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
    /// - [`crate::error::CliError::BadPicker`] for malformed names.
    /// - [`crate::error::CliError::Runtime`] for state and bundle writes.
    /// - [`crate::error::CliError::Engine`] for evaluation failures.
    /// - [`crate::error::CliError::NoDefaultProfile`] for absent defaults.
    /// - [`crate::error::CliError::NoProfile`] for absent profiles.
    pub fn execute(self) -> Result<PlanReport> {
        let path = plan_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return Err(CliError::NoDefaultProfile {
                project: self.args.project.clone(),
            });
        }
        let evaluated = Engine::new(self.stores.clone()).evaluate(&path)?;
        let desired = Manifest::build(&self.stores, evaluated.documents, evaluated.hooks)?;
        let prepared = prepare(&self.stores, desired)?;
        let text = render_prepared(&prepared);
        if let Some(output) = self.args.output.as_deref() {
            if is_named_output(output) {
                let raw = output.to_str().unwrap_or("");
                let picker: Picker = raw.parse().map_err(|_| CliError::BadPicker {
                    input: raw.to_owned(),
                })?;
                let Picker::Named(_) = picker else {
                    return Err(CliError::BadPicker {
                        input: raw.to_owned(),
                    });
                };
                Manifest::store_named(&self.stores.states(), &picker, &prepared.desired)
                    .map_err(CliError::from)?;
            } else {
                Bundler::new(&self.stores)
                    .write(&prepared.desired, output)
                    .map(|_| ())
                    .map_err(CliError::from)?;
            }
        }
        Ok(PlanReport { text })
    }
}

/// Report whether one output value is a stored state.
fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}
