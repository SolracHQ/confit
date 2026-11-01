//! Apply run
//! Project folders to disk writes with prompts.

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

/// Resolve desired documents from the project folder with explicit run fields.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::MissingProject`] for absent projects.
/// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles.
/// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable projects.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for evaluation failures.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for evaluation failures.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for unrenderable documents.
/// - [`confit_core::error::kind::ErrorKind::StoreState`] for stored states.
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable stored states.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unreadable stored states.
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
