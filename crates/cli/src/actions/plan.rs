//! Plan run
//! Profile flags into built bundles with differences.

use std::path::Path;

use confit_core::error::{
    ErrorCause, FilesystemError, SlotReason, TerminalResult, contextual, error, kind::ErrorKind,
};
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
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable projects.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated documents.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for render failures.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::PlanState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] for state writes.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] for bundle writes.
    pub fn execute(self) -> TerminalResult<PlanReport> {
        let path = plan_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return error(
                ErrorKind::MissingProject,
                contextual(FilesystemError::FileMissing).with_project(&self.args.project),
            );
        }
        let evaluated = Engine::new(self.stores.clone()).evaluate(&path)?;
        let desired = prepare::build(evaluated.documents, evaluated.hooks)?;
        let prepared = prepare(&self.stores, desired)?;
        for warning in &prepared.warnings {
            self.sinks.print_line(warning.summary());
            log::warn!(
                "diff warning: {} context: {}",
                warning.summary(),
                warning.context()
            );
        }
        let text = render_prepared(&prepared);
        if let Some(output) = self.args.output.as_deref() {
            if is_named_output(output) {
                let raw = output.to_str().unwrap_or("");
                let picker: Picker = match raw.parse() {
                    Ok(picker) => picker,
                    Err(contextual) => match contextual.cause() {
                        ErrorCause::SlotReason(_) => {
                            return Err(contextual
                                .with_slot(raw.to_owned())
                                .terminal(ErrorKind::BundleSlot));
                        }
                        _ => return error(ErrorKind::PlanState, contextual),
                    },
                };
                let Picker::Named(_) = picker else {
                    return error(
                        ErrorKind::BundleSlot,
                        contextual(SlotReason::BareSlot).with_slot(raw.to_owned()),
                    );
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
