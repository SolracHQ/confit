//! Check run
//! Profile flags into rendered differences without writes.

use confit_core::error::{FilesystemError, TerminalResult, contextual, error, kind::ErrorKind};
use confit_engine::Engine;
use confit_runtime::prepare;
use confit_store::Stores;
use confit_ui::{ToUi, UiHandles};

use crate::cli;
use crate::cli::CheckArgs;
use crate::presentation::summary;

/// Successful check run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Rendered summary text for display.
    pub text: String,
}

/// Check run from check flags to a rendered preview.
pub struct CheckRunner<'a> {
    /// Check flags for the run.
    pub args: &'a CheckArgs,
    /// Write capabilities for the run.
    pub stores: Stores,
    /// Renderer channels for prints and prompts.
    pub handles: UiHandles,
}

impl CheckRunner<'_> {
    /// Evaluate the profile and render the preview without writes.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::MissingProject`] for absent projects.
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable projects.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated documents.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for render failures.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
    /// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unreadable states.
    /// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for unreadable states.
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
