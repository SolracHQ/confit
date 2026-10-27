//! Delete run
//! Named state removal with unreferenced pruning.

use confit_core::error::{Error, ErrorCause, Outcome};
use confit_runtime::manifest::stored;
use confit_store::Stores;
use confit_store::state::Picker;

use crate::cli::DeleteArgs;

/// Outcome of one delete run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    /// The deleted state name.
    pub name: String,
    /// Pruned unreferenced pool entry count from the removal.
    pub pruned: usize,
}

/// Remove one named manifest, then prune unreferenced pool entries.
///
/// - Entries shared with remaining states survive the prune.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DeleteNonNamed`] for non named inputs.
/// - [`confit_core::error::kind::ErrorKind::DeleteSlot`] for malformed slots.
/// - [`confit_core::error::kind::ErrorKind::DeleteState`] for state removals.
/// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] for unreferenced prunes.
pub fn run(args: &DeleteArgs, stores: Stores) -> Outcome<DeleteReport> {
    if !args.name.starts_with('@') {
        return Err(Error::delete_non_named(args.name.clone()));
    }
    let picker: Picker = match args.name.parse() {
        Ok(picker) => picker,
        Err(error) => match error.root_cause() {
            ErrorCause::SlotReason(reason) => {
                return Err(Error::delete_slot(args.name.clone(), reason.clone()));
            }
            _ => return Err(Error::delete_state(error)),
        },
    };
    let Picker::Named(name) = &picker else {
        return Err(Error::delete_non_named(args.name.clone()));
    };
    let store = stores.states();
    let handle = match store.resolve(&picker) {
        Ok(handle) => handle,
        Err(error) => match error.root_cause() {
            ErrorCause::SlotReason(reason) => {
                return Err(Error::delete_slot(args.name.clone(), reason.clone()));
            }
            _ => return Err(Error::delete_state(error)),
        },
    };
    store.remove(&handle).map_err(Error::delete_state)?;
    let keep = stored::keep_set(&store).map_err(Error::delete_state)?;
    let pruned = stores.pool().prune(&keep).map_err(Error::prune_blobs)?;
    Ok(DeleteReport {
        name: name.to_string(),
        pruned,
    })
}
