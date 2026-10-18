//! Collect profile documents in destination order.

use std::collections::BTreeSet;

use crate::api::profile::Profile;
use confit_model::document::Document;

use crate::error::{EngineError, Result};

/// Reject repeated config names. Errors hold the first repeat.
///
/// # Errors
///
/// - [`EngineError::Defined`] for repeated names.
pub(super) fn check_names(profile: &Profile) -> Result<()> {
    let mut names = BTreeSet::new();
    for config in &profile.configs {
        let name = &config.name;
        if !names.insert(name.to_owned()) {
            return Err(EngineError::Defined {
                name: name.to_owned(),
            });
        }
    }
    Ok(())
}

/// Reject requirement edges showing unknown configs. Errors hold the hint.
///
/// - Configs match by exact name in profile order with targets in key order.
///
/// # Errors
///
/// - [`EngineError::Require`] for edges showing unknown configs.
pub(super) fn check_requirements(profile: &Profile) -> Result<()> {
    let mut present = BTreeSet::new();
    for config in &profile.configs {
        present.insert(config.name.clone());
    }
    for config in &profile.configs {
        let mut targets: Vec<(&String, &String)> = config.requirements.iter().collect();
        targets.sort();
        for (target, hint) in targets {
            if !present.contains(target) {
                return Err(EngineError::Require {
                    name: config.name.clone(),
                    target: target.clone(),
                    hint: Some(hint.clone()),
                });
            }
        }
    }
    Ok(())
}

/// Collect one profile's documents in destination order.
///
/// - Profile documents join first, config documents follow in order.
///
/// # Errors
///
/// - [`EngineError::Duplicate`] for repeated destinations.
pub(super) fn dedupe(profile: &Profile) -> Result<Vec<Document>> {
    let mut seen = BTreeSet::new();
    let mut placed = Vec::new();
    for document in profile.documents.iter().chain(
        profile
            .configs
            .iter()
            .flat_map(|config| config.documents.iter()),
    ) {
        if !seen.insert(document.path.clone()) {
            return Err(EngineError::Duplicate {
                display: document.path.display(),
            });
        }
        placed.push(document.clone());
    }
    placed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(placed)
}
