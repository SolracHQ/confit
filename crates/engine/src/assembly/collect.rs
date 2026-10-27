//! Collect profile documents in destination order.

use std::collections::BTreeSet;

use confit_core::error::{Error, EvalDetail, EvalError, Outcome};

use crate::api::profile::Profile;
use confit_model::document::Document;

/// Reject repeated config names.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated names.
pub(super) fn check_names(profile: &Profile) -> Outcome<()> {
    let mut names = BTreeSet::new();
    for config in &profile.configs {
        let name = config.name.as_str().to_owned();
        if !names.insert(name.clone()) {
            return Err(Error::duplicate_document(name));
        }
    }
    Ok(())
}

/// Reject requirement edges showing unknown configs.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for edges showing unknown configs.
pub(super) fn check_requirements(profile: &Profile) -> Outcome<()> {
    let mut present = BTreeSet::new();
    for config in &profile.configs {
        present.insert(config.name.as_str().to_owned());
    }
    for config in &profile.configs {
        let mut targets: Vec<(&String, &String)> = config.requirements.iter().collect();
        targets.sort();
        for (target, hint) in targets {
            if !present.contains(target) {
                return Err(Error::evaluate(
                    EvalDetail::Eval(EvalError::ConfigRequiresMissing {
                        config: config.name.as_str().to_owned(),
                        target: target.clone(),
                        hint: hint.clone(),
                    }),
                    None,
                ));
            }
        }
    }
    Ok(())
}

/// Collect one profile's documents in destination order.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated destinations.
pub(super) fn dedupe(profile: &Profile) -> Outcome<Vec<Document>> {
    let mut seen = BTreeSet::new();
    let mut placed = Vec::new();
    for document in profile.documents.iter().chain(
        profile
            .configs
            .iter()
            .flat_map(|config| config.documents.iter()),
    ) {
        if !seen.insert(document.path.clone()) {
            let display = document.path.display();
            return Err(Error::duplicate_document(display));
        }
        placed.push(document.clone());
    }
    placed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(placed)
}
