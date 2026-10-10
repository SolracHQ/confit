//! Collect profile hooks with changed gates over built routes.

use std::collections::BTreeSet;

use confit_model::document::Document as ModelDocument;
use confit_model::hook::{Hook, merge_hooks};
use confit_model::routes::Route;
use confit_surface_api::Profile;

use crate::error::{EngineError, Result};

/// Collect one profile's hooks in first-seen order.
///
/// # Errors
///
/// - [`EngineError::ChangedUnknown`] for gates showing unbuilt routes.
pub(super) fn collect_hooks(profile: &Profile, documents: &[ModelDocument]) -> Result<Vec<Hook>> {
    let mut declared = Vec::new();
    for config in profile.configs() {
        declared.extend(config.hooks().iter().cloned());
    }
    let merged = merge_hooks(declared);
    validate_changed(&merged, documents)?;
    Ok(merged)
}

/// Reject changed gates showing routes outside the built set.
///
/// # Errors
///
/// - [`EngineError::ChangedUnknown`] for gates showing unbuilt routes.
fn validate_changed(hooks: &[Hook], documents: &[ModelDocument]) -> Result<()> {
    let built: BTreeSet<Route> = documents
        .iter()
        .map(|document| document.destination.clone())
        .collect();
    let mut paths = Vec::new();
    for hook in hooks {
        if let Some(gate) = hook.requires.as_ref() {
            gate.collect_changed(&mut paths);
        }
        if let Some(gate) = hook.when.as_ref() {
            gate.collect_changed(&mut paths);
        }
        for check in &hook.checks {
            check.collect_changed(&mut paths);
        }
    }
    for path in paths {
        if !built.contains(&path) {
            return Err(EngineError::ChangedUnknown {
                route: path.display(),
            });
        }
    }
    Ok(())
}
