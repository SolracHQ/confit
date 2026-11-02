//! Writes one package onto the machine with prompts.

use std::path::PathBuf;

use confit_core::error::{
    ErrorCause, FilesystemError, TerminalResult, WithContext, contextual, error, kind::ErrorKind,
};
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::stored;
use confit_runtime::prepare;
use confit_store::Stores;
use confit_store::state::Picker;

use super::commit::{CommitRunner, WriteReport};
use crate::cli::RestoreArgs;
use confit_events::EventHandles;

/// Resolves the desired documents from one package, then commits them.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::RestoreSlot`] when a slot is malformed.
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
        let picker = match raw.parse::<Picker>().with_slot(&raw) {
            Ok(picker) => picker,
            Err(contextual) => {
                let kind = match contextual.cause() {
                    ErrorCause::SlotReason(_) => ErrorKind::RestoreSlot,
                    _ => ErrorKind::RestoreState,
                };
                return error(kind, contextual);
            }
        };
        stored::resolve(&stores.states(), &picker)?
    } else if positional
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
    {
        Bundler::new(&stores).read(positional)?
    } else {
        return error(
            ErrorKind::RestoreState,
            contextual(FilesystemError::MalformedPath).with_fs_path(positional),
        );
    };
    let prepared = prepare::prepare(&stores, desired)?;
    CommitRunner {
        prepared,
        force: args.force,
        stores,
        handles,
        hooks_dir,
    }
    .execute()
}
