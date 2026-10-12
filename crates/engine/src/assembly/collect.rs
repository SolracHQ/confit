//! Collect profile documents keyed by destination route.

use std::collections::{BTreeMap, BTreeSet};

use crate::api::profile::Profile;
use confit_model::document::Data;
use confit_model::routes::Route;

use crate::error::{EngineError, Result};

/// Reject repeated config names. The error holds the first repeat.
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
