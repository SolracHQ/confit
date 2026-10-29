//! Delete run
//! Named state removal with unreferenced pruning.

use confit_core::error::{
    ContextualCause, Error, ErrorCause, SlotReason, Terminal, TerminalResult, contextual, error,
    kind::ErrorKind,
};
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

/// Map one slot fault to its delete terminal.
fn map_slot(contextual: ContextualCause) -> Error {
    match contextual.cause() {
        ErrorCause::SlotReason(_) => contextual.terminal(ErrorKind::DeleteSlot),
        _ => contextual.terminal(ErrorKind::DeleteState),
    }
}
