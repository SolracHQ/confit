//! Check run
//! Profile flags into rendered differences without writes.

use confit_core::error::{FilesystemError, TerminalResult, contextual, error, kind::ErrorKind};
use confit_engine::Engine;
use confit_runtime::prepare;
use confit_store::Stores;

use crate::cli::{CheckArgs, resolve_profile};
use crate::presentation::summary::render_prepared;

use crate::sinks::Sinks;

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
    /// Output senders for the run.
    pub sinks: Sinks,
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
        let path = resolve_profile(&self.args.project, self.args.profile.as_deref());
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
        Ok(CheckReport { text })
    }
}
