//! Collects profile documents.

use std::collections::BTreeSet;

use confit_core::error::{
    EvalError, Source, SourceKind, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_core::suggest;

use crate::api::profile::Profile;
use confit_model::document::Document;

/// Rejects repeated config names.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when two
///   configs share a name.
pub(super) fn validate_names(profile: &Profile) -> TerminalResult<()> {
    let mut names = BTreeSet::new();
    for config in &profile.configs {
        let name = config.name.as_str().to_owned();
        if !names.insert(name.clone()) {
            let mut contextual = contextual(EvalError::DuplicateDocument).with_document(&name);
            for declaring in profile
                .configs
                .iter()
                .filter(|found| found.name.as_str() == name)
            {
                contextual = contextual.push_source(Source::new(
                    SourceKind::Config,
                    declaring.name.as_str(),
                    None,
                ));
            }
            return error(ErrorKind::DuplicateDocument, contextual);
        }
    }
    Ok(())
}

/// Rejects unknown requirements.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a requirement
///   points at an unknown config.
pub(super) fn validate_requirements(profile: &Profile) -> TerminalResult<()> {
    let mut present = BTreeSet::new();
    for config in &profile.configs {
        present.insert(config.name.as_str().to_owned());
    }
    let names: Vec<String> = present.iter().cloned().collect();
    for config in &profile.configs {
        let mut targets: Vec<(&String, &String)> = config.requirements.iter().collect();
        targets.sort();
        for (target, _hint) in targets {
            if !present.contains(target) {
                let contextual = contextual(EvalError::ConfigRequiresMissing)
                    .with_suggestions(suggest::nearby(target, &names))
                    .with_document(target)
                    .push_source(Source::new(SourceKind::Config, config.name.as_str(), None));
                return error(ErrorKind::Evaluate, contextual);
            }
        }
    }
    Ok(())
}

/// Collects one document per destination.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when two
///   documents share a destination.
pub(super) fn dedupe(profile: &Profile) -> TerminalResult<Vec<Document>> {
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
            let mut contextual = contextual(EvalError::DuplicateDocument).with_document(display);
            for declaring in profile.configs.iter().filter(|config| {
                config
                    .documents
                    .iter()
                    .any(|found| found.path == document.path)
            }) {
                let declared = declaring
                    .documents
                    .iter()
                    .find(|found| found.path == document.path)
                    .and_then(|found| found.path.trace.0.first().map(|site| site.to_string()));
                contextual = contextual.push_source(Source::new(
                    SourceKind::Config,
                    declaring.name.as_str(),
                    declared,
                ));
            }
            return error(ErrorKind::DuplicateDocument, contextual);
        }
        placed.push(document.clone());
    }
    placed.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(placed)
}
