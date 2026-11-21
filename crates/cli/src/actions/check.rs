//! Renders the differences of a profile evaluation.

use confit_core::error::{
    FilesystemError, Terminal, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_differ::diff::Diff;
use confit_engine::Engine;
use confit_events::EventHandles;
use confit_store::Stores;

use crate::cli;
use crate::cli::CheckArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// The diff that the preview renders.
    pub diff: Diff,
}

pub struct CheckRunner<'a> {
    /// The flags that pick the project and the profile.
    pub args: &'a CheckArgs,
    /// The trusted roots and content caches.
    pub stores: Stores,
    /// The UI communication channel.
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
        let desired = Engine::new(self.stores.clone(), self.handles.clone()).evaluate(&path)?;
        let previous = self.stores.states().load_applied()?;
        let diff = Diff::build(desired, previous).terminal(ErrorKind::Evaluate)?;
        self.handles.desired_preview(diff.clone());
        Ok(CheckReport { diff })
    }
}
