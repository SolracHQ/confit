//! Prepared manifests with differences and changed routes.

use std::collections::BTreeSet;

use crate::manifest::DocumentStatus;
use confit_model::routes::Route;
use confit_store::Stores;

use crate::differ::{Differ, Difference, DifferenceOrder};
use crate::error::Result;
use crate::manifest::Manifest;

/// Prepared manifests with differences and changed routes.
pub struct Prepared {
    /// Built manifest with counts.
    pub desired: Manifest,
    /// Previous manifest for lifecycle marks.
    pub previous: Manifest,
    /// Difference list behind the rendered preview.
    pub differences: Vec<Difference>,
    /// First-run flag for absent state.
    pub first_run: bool,
    /// Changed routes behind hook conditions and writes.
    pub changed: BTreeSet<Route>,
}

/// Resolve one desired manifest against previous state.
///
/// # Errors
///
/// - [`crate::error::RuntimeError::StateRead`] for state reads.
pub fn prepare(stores: &Stores, desired: Manifest) -> Result<Prepared> {
    let previous = Manifest::load(&stores.states())?;
    log_processed(&desired, &previous);
    let first_run = stores.states().is_first_run();
    let differences = difference_with(stores, first_run, &desired, &previous);
    let changed = changed_paths(&desired, &previous, &differences, first_run);
    Ok(Prepared {
        desired,
        previous,
        differences,
        first_run,
        changed,
    })
}

/// Refresh the difference list with first-run reference rules.
pub fn refresh(stores: &Stores, prepared: &Prepared) -> Vec<Difference> {
    difference_with(
        stores,
        stores.states().is_first_run(),
        &prepared.desired,
        &prepared.previous,
    )
}

/// Diff one built manifest with first-run reference rules.
fn difference_with(
    stores: &Stores,
    first_run: bool,
    built: &Manifest,
    previous: &Manifest,
) -> Vec<Difference> {
    let reference = if first_run { built } else { previous };
    let order = if first_run {
        DifferenceOrder::DiskFirst
    } else {
        DifferenceOrder::RecordedFirst
    };
    Differ {
        stores: stores.clone(),
    }
    .differences(reference, order)
}

/// Compute the changed destination routes for one apply run.
///
/// - First runs hold every desired route.
/// - Repeat runs hold plan changes and difference destinations.
fn changed_paths(
    built: &Manifest,
    previous: &Manifest,
    differences: &[Difference],
    first_run: bool,
) -> BTreeSet<Route> {
    if first_run {
        return built
            .documents
            .iter()
            .map(|document| document.destination.clone())
            .collect();
    }
    let mut out = BTreeSet::new();
    for document in &built.documents {
        if !matches!(
            document.status(&previous.documents),
            DocumentStatus::Unchanged
        ) {
            out.insert(document.destination.clone());
        }
    }
    for document in &built.documents {
        for difference in differences {
            let path = match difference {
                Difference::Key { path, .. }
                | Difference::Structured { path, .. }
                | Difference::Diff { path, .. }
                | Difference::Missing { path }
                | Difference::Unreadable { path, .. } => path,
            };
            if path == &document.destination || tree_member_of(&document.destination, path) {
                out.insert(document.destination.clone());
                break;
            }
        }
    }
    out
}

/// Report whether one tree member route nests under its destination.
fn tree_member_of(destination: &Route, path: &Route) -> bool {
    path.base() == destination.base()
        && path.relative() != destination.relative()
        && path.relative().starts_with(destination.relative())
}

/// Log finished documents with lifecycle status.
fn log_processed(built: &Manifest, previous: &Manifest) {
    for document in &built.documents {
        let status = match document.status(&previous.documents) {
            DocumentStatus::Create => "create",
            DocumentStatus::Update => "update",
            DocumentStatus::Unchanged => "unchanged",
        };
        log::debug!(
            "document processed path={} kind={} status={status}",
            document.destination.display(),
            document.data.kind().name()
        );
    }
}
