//! Writes project documents to disk with prompts.

use std::path::PathBuf;

use confit_core::error::{
    FilesystemError, Terminal, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_engine::Engine;
use confit_runtime::prepare;
use confit_store::Stores;
use confit_ui::UiHandles;

use super::commit::{CommitRunner, WriteReport};
use crate::cli;
use crate::cli::ApplyArgs;

/// Resolves the desired documents from the project folder, then commits them.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::MissingProject`] when the project folder is absent.
/// - [`confit_core::error::kind::ErrorKind::OpenProfile`] when the profile is unreadable.
/// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the project is unreadable.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the profile evaluation fails.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when the Lua evaluation fails.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a document fails to render.
/// - [`confit_core::error::kind::ErrorKind::StoreState`] when the state store fails.
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a stored manifest fails to evaluate.
pub fn run(
    args: &ApplyArgs,
    stores: Stores,
    handles: UiHandles,
    hooks_dir: PathBuf,
) -> TerminalResult<WriteReport> {
    let path = cli::resolve_profile(&args.project, args.profile.as_deref());
    if args.profile.is_none() && !path.exists() {
        return error(
            ErrorKind::MissingProject,
            contextual(FilesystemError::FileMissing).with_project(&args.project),
        );
    }
    let evaluated = Engine::new(stores.clone(), handles.clone()).evaluate(&path)?;
    let built = prepare::build(evaluated.documents, evaluated.hooks)?;
    built.persist(&stores).terminal(ErrorKind::StoreState)?;
    let prepared = prepare::prepare(&stores, built)?;
    CommitRunner {
        prepared,
        force: args.force,
        stores,
        handles,
        hooks_dir,
    }
    .execute()
}
