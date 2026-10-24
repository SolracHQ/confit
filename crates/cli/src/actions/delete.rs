//! Delete run
//! Named state removal with unreferenced pruning.

use confit_runtime::error::RuntimeError;
use confit_runtime::manifest::stored;
use confit_store::Stores;
use confit_store::state::Picker;
use confit_store::state::error::StateError;

use crate::cli::DeleteArgs;
use crate::error::{CliError, Result};

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
/// - [`CliError::BadName`] for non-`@name` values.
/// - [`CliError::BadPicker`] for malformed pickers.
/// - [`CliError::Runtime`] for state removals.
/// - [`CliError::Pool`] for unreferenced prunes.
pub fn run(args: &DeleteArgs, stores: Stores) -> Result<DeleteReport> {
    let picker: Picker = args.name.parse().map_err(|_| CliError::BadPicker {
        input: args.name.clone(),
    })?;
    let Picker::Named(name) = &picker else {
        return Err(CliError::BadName {
            input: args.name.clone(),
        });
    };
    let store = stores.states();
    let handle = store.resolve(&picker).map_err(from_state)?;
    store.remove(&handle).map_err(from_state)?;
    let keep = stored::keep_set(&store)?;
    let pruned = stores.pool().prune(&keep)?;
    Ok(DeleteReport {
        name: name.to_string(),
        pruned,
    })
}

/// Map one state failure into runtime language.
fn from_state(error: StateError) -> RuntimeError {
    match error {
        StateError::Read { path, fault } => RuntimeError::StateRead { path, fault },
        StateError::Write { path, fault } => RuntimeError::Write { path, fault },
        StateError::WriteUnknown { path, message } => {
            RuntimeError::StateWriteUnknown { path, message }
        }
        StateError::BadPick { input } => RuntimeError::BadPick { input },
    }
}
