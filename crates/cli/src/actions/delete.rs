//! Delete run
//!
//! Named slot removal with orphan pruning.

use confit_core::error::{Error, Result};

use crate::cli::DeleteArgs;

use crate::seams::Seams;

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
/// # Returns
///
/// The deleted slot name and the pruned blob count.
///
/// # Errors
///
/// Non-`@name` values, absent slots, and removal
/// failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::seams::Seams;
/// use confit_cli::cli::DeleteArgs;
/// use confit_core::fs::memory::MemoryFs;
/// use confit_core::probe::MemoryProbe;
/// use std::io::Cursor;
///
/// let args = DeleteArgs { name: "@personal".to_string() };
/// let fs = MemoryFs::new();
/// let probe = MemoryProbe::new();
/// let mut input = Cursor::new(String::new());
/// let report = confit_cli::actions::delete::run(&args, Seams::memory(&fs, &probe, &mut input));
/// assert!(matches!(report, Ok(_) | Err(_)));
/// ```
pub fn run(args: &DeleteArgs, seams: Seams<'_>) -> Result<DeleteReport> {
    let Some(name) = args.name.strip_prefix('@') else {
        return Err(Error::Plan(format!(
            "delete: '{}' reads unsupported, want '@name'; history and the current slot never delete",
            args.name
        )));
    };
    seams
        .stores
        .slots()
        .delete_named(name)
        .map_err(|error| match error {
            Error::Plan(detail) => Error::Plan(format!("delete: {detail}")),
            other => other,
        })?;
    let pruned = seams.stores.blobs().prune()?;
    Ok(DeleteReport {
        name: name.to_string(),
        pruned,
    })
}
