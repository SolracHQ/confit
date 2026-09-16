//! Apply run
//!
//! Desired documents to disk writes with prompts.

use std::path::PathBuf;

use confit_core::document::Document;
use confit_core::drift::Drift;
use confit_core::error::{Error, Result};
use confit_core::fs::{Filesystem, snapshot};
use confit_core::ids::DocPath;
use confit_core::plan::Plan;
use confit_core::store::{
    archive_previous, load_state, remove_orphans, resolve_state_file, write_documents, write_plan,
};

use crate::cli::ApplyArgs;
use crate::presentation::summary::Summary;

use super::seams::{Seams, evaluate_shared, log_processed, timed};

use confit_engine::ProgressEvent;

/// Outcome of one successful apply run.
///
/// # Examples
///
/// ```text
/// use confit_cli::actions::apply::ApplyReport;
/// use std::path::PathBuf;
///
/// let report = ApplyReport { written: 1, removed: 0, stored: PathBuf::from("previous/x.json") };
/// assert!(matches!((report.written, report.removed), (1, 0)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Counts documents written to disk.
    pub written: usize,
    /// Counts recorded orphans removed from disk.
    pub removed: usize,
    /// Holds the stored plan path backing recover.
    pub stored: PathBuf,
}

/// One apply run from desired documents to disk writes.
///
/// # Examples
///
/// ```text
/// use confit_cli::actions::apply::ApplyRunner;
/// use confit_cli::actions::seams::Seams;
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::plan::Plan;
/// use std::io::Cursor;
/// use std::path::Path;
///
/// let fs = MemoryFs::new();
/// let mut input = Cursor::new("yes\n");
/// let mut output = Vec::new();
/// let runner = ApplyRunner {
///     desired: vec![Document::new(
///         DocPath::new("note"),
///         DocumentData::Text { content: "hi".into() },
///     )],
///     previous: Plan::empty(),
///     state: None,
///     force: false,
///     preview: false,
///     seams: Seams::memory(&fs, &mut input, &mut output),
/// };
/// assert!(matches!(runner.execute(), Ok(_)));
/// assert!(fs.exists(Path::new("note")));
/// ```
pub struct ApplyRunner<'a> {
    /// Holds desired documents under writing.
    pub desired: Vec<Document>,
    /// Holds the previous plan backing drift plus counts.
    pub previous: Plan,
    /// Holds the state file gaining the new plan, `None` skips.
    pub state: Option<PathBuf>,
    /// Skips the first prompt. Drift still re-prompts.
    pub force: bool,
    /// Renders the preview through presentation first.
    pub preview: bool,
    /// Holds the injected filesystem plus prompts plus sink.
    pub seams: Seams<'a>,
}

impl<'a> ApplyRunner<'a> {
    /// Reads desired documents from flags on injected seams.
    ///
    /// A plan file runs on the file alone with no profile
    /// flag plus no engine. Otherwise the profile evaluates
    /// through the engine first.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `seams` - the injected filesystem plus prompts plus sink.
    ///
    /// # Returns
    ///
    /// The runner holding desired documents plus run flags.
    ///
    /// # Errors
    ///
    /// Evaluation plus plan load failures surface as plan
    /// or io errors.
    ///
    /// # Examples
    ///
    /// ```text,no_run
    /// use confit_cli::actions::apply::ApplyRunner;
    /// use confit_cli::actions::seams::Seams;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_core::fs::OsFs;
    /// use std::io::Cursor;
    /// use std::path::PathBuf;
    ///
    /// let args = ApplyArgs {
    ///     profile: Some(PathBuf::from("profile.lua")),
    ///     shared: confit_cli::cli::SharedArgs {
    ///         root: None,
    ///         state: None,
    ///         plugins: None,
    ///         re_fetch: false,
    ///     },
    ///     plan: None,
    ///     force: true,
    /// };
    /// let fs = OsFs;
    /// let mut input = Cursor::new(String::new());
    /// let mut output = Vec::new();
    /// let seams = Seams::memory(&fs, &mut input, &mut output);
    /// let runner = ApplyRunner::from_args(&args, seams);
    /// assert!(matches!(runner, Ok(_) | Err(_)));
    /// ```
    pub fn from_args(args: &ApplyArgs, seams: Seams<'a>) -> Result<Self> {
        if let Some(plan_file) = args.plan.as_deref() {
            seams.emit_reading_plan(plan_file);
            let file_plan = timed("apply plan load", || load_state(Some(plan_file), seams.fs))?;
            let state_file = resolve_state_file(args.shared.state.as_deref())?;
            seams.emit_reading_plan(&state_file);
            let previous = load_state(Some(&state_file), seams.fs)?;
            return Ok(Self {
                desired: file_plan.documents,
                previous,
                state: Some(state_file),
                force: args.force,
                preview: false,
                seams,
            });
        }
        let profile = args
            .profile
            .as_deref()
            .ok_or_else(|| Error::Plan("apply needs --profile while absent".to_string()))?;
        let desired = evaluate_shared(&args.shared, profile, seams.progress.clone())?;
        let state_file = resolve_state_file(args.shared.state.as_deref())?;
        seams.emit_reading_plan(&state_file);
        let previous = load_state(Some(&state_file), seams.fs)?;
        Ok(Self {
            desired,
            previous,
            state: Some(state_file),
            force: args.force,
            preview: true,
            seams,
        })
    }

    /// Reads flags plus runs the full apply flow on injected seams.
    ///
    /// # Arguments
    ///
    /// * `args` - the apply flags under running.
    /// * `seams` - the injected filesystem plus prompts plus sink.
    ///
    /// # Returns
    ///
    /// The write counts plus the stored plan path.
    ///
    /// # Errors
    ///
    /// Evaluation plus prompt plus write failures surface as
    /// plan or io errors. A non-`yes` answer aborts as a plan error.
    ///
    /// # Examples
    ///
    /// ```text,no_run
    /// use confit_cli::actions::apply::ApplyRunner;
    /// use confit_cli::actions::seams::Seams;
    /// use confit_cli::cli::ApplyArgs;
    /// use confit_core::fs::OsFs;
    /// use std::io::Cursor;
    /// use std::path::PathBuf;
    ///
    /// let args = ApplyArgs {
    ///     profile: Some(PathBuf::from("profile.lua")),
    ///     shared: confit_cli::cli::SharedArgs {
    ///         root: None,
    ///         state: None,
    ///         plugins: None,
    ///         re_fetch: false,
    ///     },
    ///     plan: None,
    ///     force: true,
    /// };
    /// let fs = OsFs;
    /// let mut input = Cursor::new(String::new());
    /// let mut output = Vec::new();
    /// let seams = Seams::memory(&fs, &mut input, &mut output);
    /// let report = ApplyRunner::run(&args, seams);
    /// assert!(matches!(report, Ok(_) | Err(_)));
    /// ```
    pub fn run(args: &ApplyArgs, seams: Seams<'a>) -> Result<ApplyReport> {
        Self::from_args(args, seams)?.execute()
    }

    /// Applies desired documents with preview plus prompts plus rotation.
    ///
    /// The preview renders through presentation. Only the literal
    /// `yes` proceeds, anything else aborts with nothing written.
    /// A fresh snapshot before writing re-prompts on drift. Success
    /// writes the state file plus one stored plan with rotation.
    ///
    /// # Returns
    ///
    /// The write counts plus the stored plan path.
    ///
    /// # Errors
    ///
    /// Build plus prompt plus write failures surface as plan or
    /// io errors. A non-`yes` answer aborts as a plan error.
    pub fn execute(mut self) -> Result<ApplyReport> {
        self.seams.emit_hashing();
        let fs: &dyn Filesystem = self.seams.fs;
        let built = Plan::build(self.desired)?;
        log_processed(&built, &self.previous);
        let snapshot = |path: &DocPath| snapshot(path, fs);
        let baseline = self.previous.drift(&snapshot);
        if self.preview {
            let report = Summary {
                built: &built,
                previous: &self.previous,
                drift: &baseline,
            };
            let text = report.render();
            self.seams
                .output
                .write_all(text.as_bytes())
                .map_err(Error::from)?;
            self.seams.output.write_all(b"\n").map_err(Error::from)?;
        }
        if !self.force && !self.seams.confirm()? {
            return Err(Error::Plan(
                "apply aborted: answer reads no 'yes'".to_string(),
            ));
        }
        let fresh = self.previous.drift(&snapshot);
        if fresh != baseline {
            for line in Drift::lines(&fresh) {
                self.seams
                    .output
                    .write_all(line.as_bytes())
                    .map_err(Error::from)?;
                self.seams.output.write_all(b"\n").map_err(Error::from)?;
            }
            if !self.seams.confirm()? {
                return Err(Error::Plan(
                    "apply aborted: answer reads no 'yes'".to_string(),
                ));
            }
        }
        let notify_written;
        let notify = if let Some(sink) = self.seams.progress.clone() {
            notify_written = move |path: &DocPath| {
                sink(ProgressEvent::DocumentWritten {
                    path: path.as_str().to_string(),
                });
            };
            Some(&notify_written as &dyn Fn(&DocPath))
        } else {
            None
        };
        write_documents(&built.documents, fs, notify)?;
        let removed = remove_orphans(&self.previous.documents, &built.documents, fs)?;
        if self.state.is_some() {
            self.seams.emit_writing_plan(built.documents.len());
        }
        if let Some(state) = self.state.as_deref() {
            write_plan(&built, Some(state), fs)?;
        }
        self.seams.emit_writing_plan(built.documents.len());
        let stored = archive_previous(&built, fs)?;
        Ok(ApplyReport {
            written: built.documents.len(),
            removed,
            stored,
        })
    }
}
