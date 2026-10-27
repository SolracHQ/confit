//! Plan run
//! Profile flags into built bundles with differences.

use std::path::Path;

use confit_core::error::{Error, ErrorCause, Failure, Outcome};
use confit_engine::Engine;
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::stored;
use confit_runtime::prepare;
use confit_store::Stores;
use confit_store::state::Picker;

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
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::BundleSlot`] for malformed slots.
    /// - [`confit_core::error::kind::ErrorKind::MissingProject`] for absent projects.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::PlanState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] for state faults.
    pub fn execute(self) -> Outcome<PlanReport> {
        let path = plan_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return Err(Error::missing_project(self.args.project.clone()));
        }
        let evaluated = Engine::new(self.stores.clone()).evaluate(&path)?;
        let desired = prepare::build(evaluated.documents, evaluated.hooks)?;
        let raw = self
            .args
            .output
            .as_deref()
            .and_then(|path| path.to_str())
            .map(str::to_owned);
        let prepared = match prepare(&self.stores, desired) {
            Ok(found) => found,
            Err(Failure::Terminal(terminal)) => return Err(terminal),
            Err(Failure::Mid(error)) => match error.root_cause() {
                ErrorCause::SlotReason(reason) => {
                    return Err(Error::bundle_slot(raw.unwrap_or_default(), reason.clone()));
                }
                _ => return Err(Error::plan_state(error)),
            },
        };
        for warning in &prepared.warnings {
            self.sinks.print_line(warning.summary());
            log::warn!(
                "diff warning: {} chain: {:?}",
                warning.summary(),
                warning.chain()
            );
        }
        let text = render_prepared(&prepared);
        if let Some(output) = self.args.output.as_deref() {
            if is_named_output(output) {
                let raw = output.to_str().unwrap_or("");
                let picker: Picker = match raw.parse() {
                    Ok(picker) => picker,
                    Err(error) => match error.root_cause() {
                        ErrorCause::SlotReason(reason) => {
                            return Err(Error::bundle_slot(raw.to_owned(), reason.clone()));
                        }
                        _ => return Err(Error::plan_state(error)),
                    },
                };
                let Picker::Named(_) = picker else {
                    return Err(Error::bundle_slot(
                        raw.to_owned(),
                        confit_core::error::SlotReason::BareSlot,
                    ));
                };
                stored::store_named(&self.stores.states(), &picker, &prepared.desired)?;
            } else {
                Bundler::new(&self.stores).write(&prepared.desired, output)?;
            }
        }
        Ok(PlanReport { text })
    }
}

/// Report whether one output value is a stored state.
fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}
