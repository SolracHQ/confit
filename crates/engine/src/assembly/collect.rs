//! Collect profile documents keyed by destination route.

use std::collections::{BTreeMap, BTreeSet};

use crate::api::profile::Profile;
use confit_model::document::Data;
use confit_model::routes::Route;

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

/// Collect one profile's documents keyed by destination route.
///
/// - Profile documents join first, config documents follow in order.
///
/// # Errors
///
/// - [`EngineError::Duplicate`] for repeated destinations.
pub(super) fn dedupe(profile: &Profile) -> Result<BTreeMap<Route, Data>> {
    let mut placed = BTreeMap::new();
    for (destination, body) in &profile.documents {
        placed.insert(destination.clone(), body.clone());
    }
    for config in &profile.configs {
        for (destination, body) in &config.documents {
            if placed.contains_key(destination) {
                return Err(EngineError::Duplicate {
                    display: destination.display(),
                });
            }
            placed.insert(destination.clone(), body.clone());
        }
    }
    Ok(placed)
}
