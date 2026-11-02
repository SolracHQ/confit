//! Renders the differences of a profile evaluation.

use confit_core::error::{FilesystemError, TerminalResult, contextual, error, kind::ErrorKind};
use confit_engine::Engine;
use confit_events::{EventHandles, ToUi};
use confit_runtime::prepare;
use confit_store::Stores;

use crate::cli;
use crate::cli::CheckArgs;
use crate::presentation::summary;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// The summary lines the check renders.
    pub text: String,
}

pub struct CheckRunner<'a> {
    /// The flags that pick the project and the profile.
    pub args: &'a CheckArgs,
    /// The stores that hold the project and the state.
    pub stores: Stores,
    /// The channels that carry the prints and the prompts.
    pub handles: EventHandles,
}

impl CheckRunner<'_> {
    /// Evaluates the profile, then renders the preview text.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::MissingProject`] when the project folder is absent.
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] when the profile is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the project is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the profile evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when a document repeats.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a document fails to render.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when the Lua evaluation fails.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the manifest version is unsupported.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when a stored manifest has no version.
    pub fn execute(self) -> TerminalResult<CheckReport> {
        let path = cli::resolve_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return error(
                ErrorKind::MissingProject,
                contextual(FilesystemError::FileMissing).with_project(&self.args.project),
            );
        }
        let evaluated = Engine::new(self.stores.clone(), self.handles.clone()).evaluate(&path)?;
        let desired = prepare::build(evaluated.documents, evaluated.hooks)?;
        let prepared = prepare::prepare(&self.stores, desired)?;
        for warning in &prepared.warnings {
            let _ = self.handles.to_ui.send(ToUi::Print(warning.summary()));
            log::warn!(
                "diff warning: {} context: {}",
                warning.summary(),
                warning.context()
            );
        }
        let text = summary::render_prepared(&prepared);
        Ok(CheckReport { text })
    }
}
