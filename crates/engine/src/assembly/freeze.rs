//! Freezes assembled documents.

use confit_core::error::{DocumentError, TerminalResult, contextual, error, kind::ErrorKind};
use confit_model::document::{Data, Document};

/// Rejects rc guards with changed leaves.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when an rc guard
///   holds a changed condition.
pub(super) fn validate_rc_guards(finished: &[Document]) -> TerminalResult<()> {
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
