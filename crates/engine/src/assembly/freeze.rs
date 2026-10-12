//! Freeze assembled payloads with rc guard checks.

use std::collections::BTreeMap;

use confit_model::document::Data;
use confit_model::routes::Route;

use crate::error::{EngineError, Result};

/// Reject rc guards with hooks-only changed leaves.
///
/// # Errors
///
/// - [`EngineError::ChangedInRc`] for guards with changed leaves.
pub(super) fn check_rc_guards(finished: &BTreeMap<Route, Data>) -> Result<()> {
    for (destination, body) in finished {
        let Data::Rc(found) = body else {
            continue;
        };
        let display = destination.display();
        for (section, entries) in [
            ("profile", &found.profile),
            ("config", &found.config),
            ("final", &found.final_entries),
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
