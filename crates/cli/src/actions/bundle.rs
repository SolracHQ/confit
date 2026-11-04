//! Writes a profile evaluation to a bundle file or a named state.

use std::path::Path;

use confit_bundler::Bundler;
use confit_core::error::{
    ErrorCause, FilesystemError, StateReason, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_engine::Engine;
use confit_events::EventHandles;
use confit_store::Stores;
use confit_store::state::Picker;

use crate::cli;
use crate::cli::BundleArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleReport {
    /// The bundle path or the named state as display text.
    pub text: String,
}

pub struct BundleRunner<'a> {
    /// The flags that pick the project, the profile, and the output.
    pub args: &'a BundleArgs,
    /// The stores that hold the project and the state.
    pub stores: Stores,
    /// The channels that carry the prints and the prompts.
    pub handles: EventHandles,
}

impl BundleRunner<'_> {
    /// Evaluates the profile, then writes the bundle file or the named state.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::BundleState`] when the state picker text is malformed.
    /// - [`confit_core::error::kind::ErrorKind::BundleState`] when a state fails.
    /// - [`confit_core::error::kind::ErrorKind::MissingProject`] when the project folder is absent.
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] when the profile is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the project is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the profile evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when a document repeats.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a document fails to render.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when the Lua evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::StoreState`] when the state write fails.
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] when the bundle write fails.
    pub fn execute(self) -> TerminalResult<BundleReport> {
        let path = cli::resolve_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return error(
                ErrorKind::MissingProject,
                contextual(FilesystemError::FileMissing).with_project(&self.args.project),
            );
        }
        let desired = Engine::new(self.stores.clone(), self.handles.clone()).evaluate(&path)?;
        let output = self.args.output.as_path();
        if is_named_output(output) {
            let raw = output.to_str().unwrap_or("");
            let picker: Picker = match raw.parse() {
                Ok(picker) => picker,
                Err(contextual) => match contextual.cause() {
                    ErrorCause::StateReason(_) => {
                        return Err(contextual
                            .with_state(raw.to_owned())
                            .terminal(ErrorKind::BundleState));
                    }
                    _ => return error(ErrorKind::BundleState, contextual),
                },
            };
            let Picker::Named(name) = &picker else {
                return error(
                    ErrorKind::BundleState,
                    contextual(StateReason::BareState).with_state(raw.to_owned()),
                );
            };
            self.stores.states().save_named(name, &desired)?;
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

/// Returns whether the output path points at a stored state.
fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}
