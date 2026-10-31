//! Bundle run
//! Profile flags into written bundle files and named states.

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

use crate::cli::{BundleArgs, resolve_profile};

/// Successful bundle run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleReport {
    /// Bundle path or named slot for display.
    pub text: String,
}

/// Bundle run from bundle flags to a written artifact.
pub struct BundleRunner<'a> {
    /// Bundle flags for the run.
    pub args: &'a BundleArgs,
    /// Write capabilities for the run.
    pub stores: Stores,
}

impl BundleRunner<'_> {
    /// Evaluate the profile and write the bundle file or named state.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::BundleSlot`] for malformed slots.
    /// - [`confit_core::error::kind::ErrorKind::BundleState`] for state faults.
    /// - [`confit_core::error::kind::ErrorKind::MissingProject`] for absent projects.
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable projects.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated documents.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for render failures.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] for state writes.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] for bundle writes.
    pub fn execute(self) -> TerminalResult<BundleReport> {
        let path = resolve_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return error(
                ErrorKind::MissingProject,
                contextual(FilesystemError::FileMissing).with_project(&self.args.project),
            );
        }
        let evaluated = Engine::new(self.stores.clone()).evaluate(&path)?;
        let desired = prepare::build(evaluated.documents, evaluated.hooks)?;
        let output = self.args.output.as_path();
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
                    _ => return error(ErrorKind::BundleState, contextual),
                },
            };
            let Picker::Named(_) = picker else {
                return error(
                    ErrorKind::BundleSlot,
                    contextual(SlotReason::BareSlot).with_slot(raw.to_owned()),
                );
            };
            stored::store_named(&self.stores.states(), &picker, &desired)?;
            Ok(BundleReport {
                text: raw.to_owned(),
            })
        } else {
            let dest = Bundler::new(&self.stores).write(&desired, output)?;
            Ok(BundleReport {
                text: dest.display().to_string(),
            })
        }
    }
}

/// Report whether one output value is a stored state.
fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}
