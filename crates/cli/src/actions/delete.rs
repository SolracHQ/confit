//! Delete run
//!
//! Named slot removal with orphan pruning.

use confit_core::error::{Error, Result};
use confit_core::store::{prune_blobs, resolve_named_plan};

use crate::cli::DeleteArgs;

use super::seams::Seams;

/// Outcome of one delete run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeleteReport {
    /// Holds the deleted slot name without the `@` sigil.
    pub name: String,
    /// Counts pool blobs pruned as orphaned by the removal.
    pub pruned: usize,
}

/// One delete run from flags on injected seams.
///
/// # Examples
///
/// ```rust,no_run
/// use confit_cli::actions::delete::DeleteRunner;
/// use confit_cli::actions::seams::Seams;
/// use confit_cli::cli::DeleteArgs;
/// use confit_cli::fs::OsFs;
/// use std::io::Cursor;
///
/// let args = DeleteArgs { name: "@personal".to_string() };
/// let fs = OsFs;
/// let mut input = Cursor::new(String::new());
/// let mut output = Vec::new();
/// let report = DeleteRunner::run(&args, Seams::memory(&fs, &mut input, &mut output));
/// assert!(matches!(report, Ok(_) | Err(_)));
/// ```
pub struct DeleteRunner<'a> {
    /// Holds the delete flags under running.
    pub args: &'a DeleteArgs,
    /// Holds the injected filesystem plus output plus sink.
    pub seams: Seams<'a>,
}

impl<'a> DeleteRunner<'a> {
    /// Reads flags plus runs the full delete flow on injected seams.
    pub fn run(args: &'a DeleteArgs, seams: Seams<'a>) -> Result<DeleteReport> {
        Self { args, seams }.execute()
    }

    /// Removes one named manifest, then prunes orphan pool blobs.
    ///
    /// Only `@name` values delete. History plus the current
    /// slot never delete. Blobs shared with remaining slots
    /// stay kept through the prune.
    ///
    /// # Returns
    ///
    /// The deleted slot name plus the pruned blob count.
    ///
    /// # Errors
    ///
    /// Non-`@name` values plus absent slots plus removal
    /// failures surface as plan or io errors.
    pub fn execute(self) -> Result<DeleteReport> {
        let args = self.args;
        let seams = self.seams;
        let Some(name) = args.name.strip_prefix('@') else {
            return Err(Error::Plan(format!(
                "delete: '{}' reads unsupported, want '@name'; history and the current slot never delete",
                args.name
            )));
        };
        let path = resolve_named_plan(name)?;
        if !seams.fs.exists(&path) {
            return Err(Error::Plan(format!("delete: '@{name}' reads absent")));
        }
        seams.fs.remove(&path).map_err(Error::from)?;
        let pruned = prune_blobs(seams.fs)?;
        Ok(DeleteReport {
            name: name.to_string(),
            pruned,
        })
    }
}
