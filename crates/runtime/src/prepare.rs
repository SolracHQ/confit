//! Prepared manifests with differences and changed routes.

use std::collections::BTreeSet;

use confit_core::error::kind::ErrorKind;
use confit_core::error::{DiffWarning, Terminal, TerminalResult};
use confit_model::document::Data;
use confit_model::manifest::Manifest;
use confit_model::routes::Route;
use confit_store::Stores;

use crate::differ::{Differ, Difference, DifferenceOrder};
use crate::status::{self, DocumentStatus};

/// One resolved manifest.
pub struct Prepared {
    /// The built manifest.
    pub desired: Manifest,
    /// The manifest from the previous run.
    pub previous: Manifest,
    /// The differences that the preview shows.
    pub differences: Vec<Difference>,
    /// The warnings that failed states produce without stopping the run.
    pub warnings: Vec<DiffWarning>,
    /// Whether the run starts with no stored state.
    pub first_run: bool,
    /// The routes that changed, for hook conditions and writes.
    pub changed: BTreeSet<Route>,
}

/// Resolves one desired manifest against the stored state.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::CorruptManifest`] when a stored state is unreadable.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a difference reports an evaluation fault.
pub fn prepare(stores: &Stores, desired: Manifest) -> TerminalResult<Prepared> {
    let previous = stores.states().load_applied()?;
    log_processed(&desired, &previous);
    let first_run = stores.states().is_first_run();
    let (differences, warnings) = difference_with(stores, first_run, &desired, &previous)?;
    let changed = changed_paths(&desired, &previous, &differences, first_run);
    Ok(Prepared {
        desired,
        previous,
        differences,
        warnings,
        first_run,
        changed,
    })
}

/// Refreshes the differences with the first-run reference rules.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a difference reports an evaluation fault.
pub fn refresh(
    stores: &Stores,
    prepared: &Prepared,
) -> TerminalResult<(Vec<Difference>, Vec<DiffWarning>)> {
    let found = difference_with(
        stores,
        stores.states().is_first_run(),
        &prepared.desired,
        &prepared.previous,
    )?;
    Ok(found)
}

/// Diffs one built manifest against the reference under the first-run rules.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a difference reports an evaluation fault.
fn difference_with(
    stores: &Stores,
    first_run: bool,
    built: &Manifest,
    previous: &Manifest,
) -> TerminalResult<(Vec<Difference>, Vec<DiffWarning>)> {
    let reference = if first_run { built } else { previous };
    let order = if first_run {
        DifferenceOrder::DiskFirst
    } else {
        DifferenceOrder::RecordedFirst
    };
    let differ = Differ {
        stores: stores.clone(),
    };
    differ
        .differences(reference, order)
        .terminal(ErrorKind::Evaluate)
}

/// Computes the changed routes for one apply run.
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
            .map(|document| document.path.clone())
            .collect();
    }
    let mut out = BTreeSet::new();
    for document in &built.documents {
        if !matches!(
            status::status(document, &previous.documents),
            DocumentStatus::Unchanged
        ) {
            out.insert(document.path.clone());
        }
    }
    for document in &built.documents {
        for difference in differences {
            let path = match difference {
                Difference::Key { path, .. }
                | Difference::Structured { path, .. }
                | Difference::Diff { path, .. }
                | Difference::Missing { path }
                | Difference::Blocked { path }
                | Difference::Unreadable { path, .. } => path,
            };
            if path == &document.path || tree_member_of(&document.path, path) {
                out.insert(document.path.clone());
                break;
            }
        }
    }
    out
}

/// Returns whether one tree member route nests under its destination.
fn tree_member_of(destination: &Route, path: &Route) -> bool {
    path.base() == destination.base()
        && path.relative_path() != destination.relative_path()
        && path
            .relative_path()
            .starts_with(destination.relative_path())
}

/// Logs the documents that finished with their lifecycle status.
fn log_processed(built: &Manifest, previous: &Manifest) {
    for document in &built.documents {
        let status = match status::status(document, &previous.documents) {
            DocumentStatus::Create => "create",
            DocumentStatus::Update => "update",
            DocumentStatus::Unchanged => "unchanged",
        };
        log::debug!(
            "document processed path={} kind={} status={status}",
            document.path.display(),
            match &document.data {
                Data::Structured(_) => "structured",
                Data::Text(_) => "text",
                Data::Link(_) => "link",
                Data::Rc(_) => "rc",
                Data::Opaque(_) => "opaque",
                Data::Tree(_) => "tree",
            }
        );
    }
}
