//! Writes one package onto the machine with prompts.

use std::path::PathBuf;

use confit_bundler::Bundler;
use confit_core::error::{
    FilesystemError, Terminal, TerminalResult, WithContext, contextual, error, kind::ErrorKind,
};
use confit_differ::diff::Diff;
use confit_events::EventHandles;
use confit_store::{Stores, state::Picker};

use super::commit::{Applier, WriteReport};
use crate::cli::RestoreArgs;

/// Resolves the desired documents from one package, then commits them.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::RestoreState`] when the state picker text is malformed.
/// - [`confit_core::error::kind::ErrorKind::RestoreState`] when a state fails or the source is not a package.
/// - [`confit_core::error::kind::ErrorKind::ReadBundle`] when a bundle is unreadable.
/// - [`confit_core::error::kind::ErrorKind::UnpackMember`] when a bundle member is unreadable.
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored manifest is unreadable.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] when the manifest version is unsupported.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] when a stored manifest has no version.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when the diff fails.
pub fn run(
    args: &RestoreArgs,
    stores: Stores,
    handles: EventHandles,
    hooks_dir: PathBuf,
) -> TerminalResult<WriteReport> {
    let positional = args.source.as_path();
    let raw = positional.to_str().unwrap_or("").to_owned();
    let desired = if raw.starts_with('@') || raw.starts_with('%') {
        let picker = match raw.parse::<Picker>().with_state(&raw) {
            Ok(picker) => picker,
            Err(contextual) => return error(ErrorKind::RestoreState, contextual),
        };
        stores.states().load_state(&picker)?
    } else if positional
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
    {
        Bundler::new(&stores, handles.clone()).read(positional)?
    } else {
        return error(
            ErrorKind::RestoreState,
            contextual(FilesystemError::MalformedPath).with_fs_path(positional),
        );
    };
    let previous = stores.states().load_applied()?;
    let diff = Diff::build(desired, previous).terminal(ErrorKind::Evaluate)?;
    Applier {
        diff,
        force: args.force,
        stores,
        handles,
        hooks_dir,
    }
    .execute()
}
