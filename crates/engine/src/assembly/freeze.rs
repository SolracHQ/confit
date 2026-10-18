//! Freeze assembled documents with rc guard checks.

use confit_model::document::{Data, Document};

use crate::error::{EngineError, Result};

/// Reject rc guards with hooks-only changed leaves.
///
/// # Errors
///
/// - [`EngineError::ChangedInRc`] for guards with changed leaves.
pub(super) fn check_rc_guards(finished: &[Document]) -> Result<()> {
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
                    return Err(EngineError::ChangedInRc {
                        display: display.clone(),
                        section: section.to_owned(),
                    });
                }
            }
        }
    }
    Ok(())
}
