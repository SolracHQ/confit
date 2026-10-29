//! Freeze assembled documents with rc guard checks.

use confit_core::error::{DocumentError, TerminalResult, contextual, error, kind::ErrorKind};
use confit_model::document::{Data, Document};

/// Reject rc guards with hooks-only changed leaves.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for guards with changed leaves.
pub(super) fn check_rc_guards(finished: &[Document]) -> TerminalResult<()> {
    for document in finished {
        let Data::Rc(found) = &document.data else {
            continue;
        };
        let display = document.path.display();
        for entries in [&found.profile, &found.config, &found.r#final] {
            for entry in entries {
                if entry
                    .when()
                    .is_some_and(|condition| condition.holds_changed())
                {
                    return error(
                        ErrorKind::RenderManifest,
                        contextual(DocumentError::RenderFailed).with_document(&display),
                    );
                }
            }
        }
    }
    Ok(())
}
