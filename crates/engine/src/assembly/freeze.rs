//! Freeze assembled documents with rc guard checks.

use confit_core::error::{DocumentError, Error, ErrorContext, Outcome};
use confit_model::document::{Data, Document};

/// Reject rc guards with hooks-only changed leaves.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for guards with changed leaves.
pub(super) fn check_rc_guards(finished: &[Document]) -> Outcome<()> {
    for document in finished {
        let Data::Rc(found) = &document.data else {
            continue;
        };
        let display = document.path.display();
        for (section, entries) in [
            ("profile", &found.profile),
            ("config", &found.config),
            ("final", &found.r#final),
        ] {
            for entry in entries {
                if entry
                    .when()
                    .is_some_and(|condition| condition.holds_changed())
                {
                    return Err(Error::render_manifest(
                        ErrorContext::from(DocumentError::RenderFailed {
                            reason: format!(
                                "documents '{display}' {section} hold 'changed' (hooks only)"
                            ),
                        })
                        .route(display.clone()),
                    ));
                }
            }
        }
    }
    Ok(())
}
