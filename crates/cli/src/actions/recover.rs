//! Recover run
//!
//! Stored plan listing plus re-apply.

use confit_core::error::{Error, Result};
use confit_core::store::{default_state_path, load_state, resolve_previous_dir, stored_entries};

use crate::cli::RecoverArgs;

use super::apply::{ApplyReport, ApplyRunner};
use super::seams::Seams;

/// One recover run from flags on injected seams.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::recover::RecoverRunner;
/// use confit_cli::actions::seams::Seams;
/// use confit_cli::cli::RecoverArgs;
/// use confit_core::fs::MemoryFs;
/// use std::io::Cursor;
///
/// let fs = MemoryFs::new();
/// let mut input = Cursor::new(String::new());
/// let mut output = Vec::new();
/// let args = RecoverArgs { index: None, force: false };
/// let runner = RecoverRunner { args: &args, seams: Seams::memory(&fs, &mut input, &mut output) };
/// assert!(matches!(runner.execute(), Ok(None)));
/// ```
pub struct RecoverRunner<'a> {
    /// Holds the recover flags under running.
    pub args: &'a RecoverArgs,
    /// Holds the injected filesystem plus prompts plus sink.
    pub seams: Seams<'a>,
}

impl RecoverRunner<'_> {
    /// Lists stored plans or re-applies the picked index.
    ///
    /// No index lists stored plans with timestamp. An index
    /// re-applies the picked stored plan through the apply
    /// flow with preview plus prompts plus rotation.
    ///
    /// # Returns
    ///
    /// The apply report for a picked index, `None` for a listing.
    ///
    /// # Errors
    ///
    /// Resolution plus prompt plus write failures surface as
    /// plan or io errors. Unknown indices fail as plan errors.
    pub fn execute(self) -> Result<Option<ApplyReport>> {
        let dir = resolve_previous_dir()?;
        let entries = stored_entries(&dir, self.seams.fs)?;
        let Some(index) = self.args.index else {
            for (position, (_, stored)) in entries.iter().enumerate() {
                let line = format!("{position} @ {}\n", stored.created_at);
                self.seams
                    .output
                    .write_all(line.as_bytes())
                    .map_err(Error::from)?;
            }
            return Ok(None);
        };
        let (_, stored) = entries.get(index).cloned().ok_or_else(|| {
            Error::Plan(format!(
                "recover: index {index} reads out of range, holding {} stored plans",
                entries.len()
            ))
        })?;
        log::debug!("recover index={index}");
        let state_file = default_state_path()?;
        self.seams.emit_reading_plan(&state_file);
        let previous = load_state(Some(state_file.as_path()), self.seams.fs)?;
        let report = ApplyRunner {
            plan: stored,
            previous,
            state: Some(state_file),
            force: self.args.force,
            preview: true,
            seams: self.seams,
        }
        .execute()?;
        Ok(Some(report))
    }
}
