//! Apply run
//! Project folders to disk writes with prompts.

use std::io::BufRead;
use std::path::PathBuf;

use confit_core::error::{
    FilesystemError, Terminal, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_engine::Engine;
use confit_runtime::{prepare, prepare::build};
use confit_store::Stores;

use super::commit::{CommitRunner, WriteReport};
use crate::cli::{ApplyArgs, resolve_profile};
use crate::sinks::Sinks;

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
    input: &mut dyn BufRead,
    stores: Stores,
    sinks: Sinks,
    hooks_dir: PathBuf,
) -> TerminalResult<WriteReport> {
    let path = resolve_profile(&args.project, args.profile.as_deref());
    if args.profile.is_none() && !path.exists() {
        return error(
            ErrorKind::MissingProject,
            contextual(FilesystemError::FileMissing).with_project(&args.project),
        );
    }
    let evaluated = Engine::new(stores.clone()).evaluate(&path)?;
    let built = build(evaluated.documents, evaluated.hooks)?;
    built.persist(&stores).terminal(ErrorKind::StoreState)?;
    let prepared = prepare(&stores, built)?;
    CommitRunner {
        prepared,
        force: args.force,
        input,
        stores,
        sinks,
        hooks_dir,
    }
    .execute()
}
