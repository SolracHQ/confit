//! Delete run
//!
//! Named slot removal with orphan pruning.

use confit_store::Stores;

use crate::cli::DeleteArgs;
use crate::error::{CliError, Result};

/// Outcome of one delete run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    /// Holds the deleted slot name without the `@` sigil.
    pub name: String,
    /// Counts pool blobs pruned as orphaned by the removal.
    pub pruned: usize,
}

/// Removes one named manifest, then prunes orphan pool blobs.
///
/// Takes one `@name` value. Blobs shared with remaining slots
/// stay kept through the prune.
///
/// # Errors
///
/// - [`CliError::BadName`] for non-`@name` values.
/// - [`CliError::Slot`] for slot removals.
/// - [`CliError::Blob`] for orphan prunes.
pub fn run(args: &DeleteArgs, stores: Stores) -> Result<DeleteReport> {
    let Some(name) = args.name.strip_prefix('@') else {
        return Err(CliError::BadName {
            input: args.name.clone(),
        });
    };
    stores.slots().delete_named(name)?;
    let pruned = stores.blobs().prune()?;
    Ok(DeleteReport {
        name: name.to_string(),
        pruned,
    })
}
