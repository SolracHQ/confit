//! Writes a profile evaluation to a bundle file or a named state.

use std::path::Path;

use confit_bundler::Bundler;
use confit_core::error::{
    ErrorCause, FilesystemError, StateReason, Terminal, TerminalResult, contextual, error,
    kind::ErrorKind,
};
use confit_core::version::VERSION;
use confit_differ::diff::Diff;
use confit_engine::Engine;
use confit_events::{EventHandles, protocol::Phase};
use confit_model::manifest::Manifest;
use confit_store::{Stores, state::Picker};

use crate::actions::ArtifactReport;
use crate::cli::{self, BundleArgs};

pub struct BundleRunner<'a> {
    /// The flags that pick the project, the profile, and the output.
    pub args: &'a BundleArgs,
    /// The trusted roots and content caches.
    pub stores: Stores,
    /// The UI communication channel.
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
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the preview diff fails.
    pub fn execute(self) -> TerminalResult<ArtifactReport> {
        let path = cli::resolve_profile(&self.args.project, self.args.profile.as_deref());
        if self.args.profile.is_none() && !path.exists() {
            return error(
                ErrorKind::MissingProject,
                contextual(FilesystemError::FileMissing).with_project(&self.args.project),
            );
        }
        let desired = Engine::new(self.stores.clone(), self.handles.clone()).evaluate(&path)?;
        let output = self.args.output.as_path();
        let rendered = if is_named_output(output) {
            self.handles.phase_started(Phase::Persisting);
            self.save_named_state(&desired, output)?
        } else {
            self.handles.phase_started(Phase::Packaging);
            self.write_file(&desired, output)?
        };
        let diff = preview_diff(&desired)?;
        self.handles.desired_preview(diff);
        Ok(ArtifactReport {
            output: rendered,
            manifest: desired,
        })
    }

    /// Saves the manifest under the `@name` output with pack events.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::BundleState`] when the state picker text is malformed.
    /// - [`confit_core::error::kind::ErrorKind::BundleState`] when a state fails.
    fn save_named_state(&self, desired: &Manifest, output: &Path) -> TerminalResult<String> {
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
        self.stores.states().save_named(name, desired)?;
        Ok(raw.to_owned())
    }

    /// Writes the manifest to the bundle file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::WriteBundle`] when the bundle write fails.
    fn write_file(&self, desired: &Manifest, output: &Path) -> TerminalResult<String> {
        let dest = Bundler::new(&self.stores, self.handles.clone()).write(desired, output)?;
        Ok(dest.display().to_string())
    }
}

/// Returns whether the output path points at a stored state.
fn is_named_output(raw: &Path) -> bool {
    raw.to_str().is_some_and(|text| text.starts_with('@'))
}

/// Builds the bundle preview against an empty baseline.
///
/// - The baseline contains no documents.
/// - The baseline carries the binary version.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the
///   diff build fails.
fn preview_diff(desired: &Manifest) -> TerminalResult<Diff> {
    let previous = Manifest {
        version: VERSION,
        documents: Vec::new(),
        hooks: Vec::new(),
    };
    Diff::build(desired.clone(), previous).terminal(ErrorKind::Evaluate)
}
