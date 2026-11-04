//! Removes one named state with its unreferenced pruning.

use confit_core::error::{
    StateReason, Terminal, TerminalResult, contextual, error, kind::ErrorKind,
};
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
/// - [`confit_core::error::kind::ErrorKind::DeleteState`] when the state picker text is malformed.
/// - [`confit_core::error::kind::ErrorKind::DeleteState`] when the state removal fails.
/// - [`confit_core::error::kind::ErrorKind::PruneBlobs`] when the unreferenced prune fails.
pub fn run(args: &DeleteArgs, stores: Stores) -> TerminalResult<DeleteReport> {
    if !args.name.starts_with('@') {
        return error(
            ErrorKind::DeleteNonNamed,
            contextual(StateReason::BareState).with_state(&args.name),
        );
    }
    let picker: Picker = match args.name.parse() {
        Ok(picker) => picker,
        Err(contextual) => {
            return Err(contextual
                .with_state(&args.name)
                .terminal(ErrorKind::DeleteState));
        }
    };
    let Picker::Named(name) = &picker else {
        return error(
            ErrorKind::DeleteNonNamed,
            contextual(StateReason::BareState).with_state(&args.name),
        );
    };
    let store = stores.states();
    match store.remove_state(&picker) {
        Ok(()) => {}
        Err(contextual) => {
            return Err(contextual
                .with_state(&args.name)
                .terminal(ErrorKind::DeleteState));
        }
    }
    let pruned = stores
        .prune_unreferenced()
        .terminal(ErrorKind::PruneBlobs)?;
    Ok(DeleteReport {
        name: name.to_string(),
        pruned,
    })
}
