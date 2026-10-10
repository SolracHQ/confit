//! Prepared manifests with drift and changed routes.

use std::collections::BTreeSet;

use confit_model::document::DocumentStatus;
use confit_model::drift::{Drift, DriftOrder};
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use confit_store::Stores;

use crate::differ::Differ;
use crate::error::Result;

/// Prepared manifests with drift and changed routes.
pub struct Prepared {
    /// Built manifest with counts.
    pub desired: Manifest,
    /// Previous manifest for lifecycle marks.
    pub previous: Manifest,
    /// Drift list behind the rendered preview.
    pub drifts: Vec<Drift>,
    /// First-run flag for an absent state slot.
    pub first_run: bool,
    /// Changed routes behind hook conditions and writes.
    pub changed: BTreeSet<Route>,
}

/// Resolve one desired manifest against previous state.
///
/// # Errors
///
/// - [`crate::error::RuntimeError::Slot`] for slot reads.
pub fn prepare(stores: &Stores, desired: Manifest) -> Result<Prepared> {
    let previous = stores.slots().load()?;
    log_processed(&desired, &previous);
    let first_run = stores.slots().is_first_run();
    let drifts = drift_with(stores, first_run, &desired, &previous);
    let changed = changed_paths(&desired, &previous, &drifts, first_run);
    Ok(Prepared {
        desired,
        previous,
        drifts,
        first_run,
        changed,
    })
}

/// Refresh the drift list with first-run reference rules.
pub fn refresh(stores: &Stores, prepared: &Prepared) -> Vec<Drift> {
    drift_with(
        stores,
        stores.slots().is_first_run(),
        &prepared.desired,
        &prepared.previous,
    )
}

/// Diff one built manifest with first-run reference rules.
fn drift_with(
    stores: &Stores,
    first_run: bool,
    built: &Manifest,
    previous: &Manifest,
) -> Vec<Drift> {
    let reference = if first_run { built } else { previous };
    let order = if first_run {
        DriftOrder::DiskFirst
    } else {
        DriftOrder::RecordedFirst
    };
    Differ {
        stores: stores.clone(),
    }
    .drift(reference, order)
}

/// Compute the changed destination routes for one apply run.
///
/// - First runs hold every desired route.
/// - Steady runs hold plan changes and drifted destinations.
fn changed_paths(
    built: &Manifest,
    previous: &Manifest,
    drifts: &[Drift],
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
        if !matches!(document.status(previous), DocumentStatus::Unchanged) {
            out.insert(document.destination.clone());
        }
    }
    for document in &built.documents {
        let prefix = format!("{}/", document.destination.display());
        for drift in drifts {
            let path = drift.path().display();
            if path == document.destination.display() || path.starts_with(&prefix) {
                out.insert(document.destination.clone());
                break;
            }
        }
    }
    out
}

/// Log finished documents with lifecycle status.
fn log_processed(built: &Manifest, previous: &Manifest) {
    for document in &built.documents {
        let status = match document.status(previous) {
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
