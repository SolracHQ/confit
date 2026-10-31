//! Restore run
//! Packages to disk writes with prompts.

use std::io::BufRead;
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
use crate::sinks::Sinks;

/// Resolve desired documents from one package with explicit run fields.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::RestoreSlot`] for malformed slots.
/// - [`confit_core::error::kind::ErrorKind::RestoreState`] for state faults and non-package sources.
/// - [`confit_core::error::kind::ErrorKind::ReadBundle`] for unreadable bundles.
/// - [`confit_core::error::kind::ErrorKind::UnpackMember`] for unreadable bundle members.
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] for unreadable states.
/// - [`confit_core::error::kind::ErrorKind::ManifestVersion`] for unsupported states.
/// - [`confit_core::error::kind::ErrorKind::ManifestNoVersion`] for versionless states.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for diff faults.
pub fn run(
    args: &RestoreArgs,
    input: &mut dyn BufRead,
    stores: Stores,
    sinks: Sinks,
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
    let prepared = prepare(&stores, desired)?;
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
