//! Removes one named state with its unreferenced pruning.

use confit_core::error::{
    ContextualCause, Error, ErrorCause, SlotReason, Terminal, TerminalResult, contextual, error,
    kind::ErrorKind,
};
use confit_runtime::manifest::stored;
use confit_store::Stores;
use confit_store::state::Picker;

use crate::cli::DeleteArgs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    /// The deleted state name.
    pub name: String,
    /// The number of unreferenced pool entries the removal pruned.
    pub pruned: usize,
}

/// Removes one named manifest, then prunes the unreferenced pool entries.
///
/// - An entry shared with a remaining state survives the prune.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DeleteNonNamed`] when the name is not a named state.
/// - [`confit_core::error::kind::ErrorKind::DeleteSlot`] when a slot is malformed.
/// - [`confit_core::error::kind::ErrorKind::DeleteState`] when the state removal fails.
/// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] when the unreferenced prune fails.
pub fn run(args: &DeleteArgs, stores: Stores) -> TerminalResult<DeleteReport> {
    if !args.name.starts_with('@') {
        return error(
            ErrorKind::DeleteNonNamed,
            contextual(SlotReason::BareSlot).with_slot(&args.name),
        );
    }
    let picker: Picker = match args.name.parse() {
        Ok(picker) => picker,
        Err(contextual) => return Err(map_slot(contextual.with_slot(&args.name))),
    };
    let Picker::Named(name) = &picker else {
        return error(
            ErrorKind::DeleteNonNamed,
            contextual(SlotReason::BareSlot).with_slot(&args.name),
        );
    };
    let store = stores.states();
    let handle = match store.resolve(&picker) {
        Ok(handle) => handle,
        Err(contextual) => return Err(map_slot(contextual.with_slot(&args.name))),
    };
    store.remove(&handle).terminal(ErrorKind::DeleteState)?;
    let keep = stored::keep_set(&store).terminal(ErrorKind::DeleteState)?;
    let pruned = stores.pool().prune(&keep).terminal(ErrorKind::PruneBlobs)?;
    Ok(DeleteReport {
        name: name.to_string(),
        pruned,
    })
}

/// Maps one slot fault to the delete error kind.
fn map_slot(contextual: ContextualCause) -> Error {
    match contextual.cause() {
        ErrorCause::SlotReason(_) => contextual.terminal(ErrorKind::DeleteSlot),
        _ => contextual.terminal(ErrorKind::DeleteState),
    }
}
