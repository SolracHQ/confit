//! Collect profile hooks with changed conditions over built routes.

use std::collections::{BTreeMap, BTreeSet};

use crate::api::profile::Profile;
use confit_model::document::Data;
use confit_model::hook::{Hook, merge_hooks};
use confit_model::routes::Route;

use crate::error::{EngineError, Result};

/// Collect one profile's hooks in first-seen order.
///
/// # Errors
///
/// - [`EngineError::ChangedUnknown`] for conditions showing unbuilt routes.
pub(super) fn collect_hooks(
    profile: &Profile,
    documents: &BTreeMap<Route, Data>,
) -> Result<Vec<Hook>> {
    let mut declared = Vec::new();
    for config in &profile.configs {
        declared.extend(config.hooks.iter().cloned());
    }
    let merged = merge_hooks(declared);
    validate_changed(&merged, documents)?;
    Ok(merged)
}

/// Reject changed conditions showing routes outside the built set.
///
/// # Errors
///
/// - [`EngineError::ChangedUnknown`] for conditions showing unbuilt routes.
fn validate_changed(hooks: &[Hook], documents: &BTreeMap<Route, Data>) -> Result<()> {
    let built: BTreeSet<Route> = documents.keys().cloned().collect();
    let mut paths = Vec::new();
    for hook in hooks {
        if let Some(condition) = hook.requires.as_ref() {
            condition.collect_changed(&mut paths);
        }
        if let Some(condition) = hook.when.as_ref() {
            condition.collect_changed(&mut paths);
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
