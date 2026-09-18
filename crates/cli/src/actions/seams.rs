//! Seams
//!
//! Injected effects shared by command runners.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use confit_core::error::{Error, Result};
use confit_core::fs::Filesystem;

use crate::fs::OsFs;
use confit_core::plan::{Bundle, DocumentStatus};

use confit_engine::{ProgressCallback, ProgressEvent};

use crate::actions::hooks::HookRunner;
use crate::cli::{SharedArgs, resolve_plugins, resolve_root};

/// Host filesystem under sharing by host seams.
static HOST_FS: OsFs = OsFs;

/// Injected effects under one command run.
///
/// Host runs pass stdio locks. Tests pass memory fakes.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::seams::Seams;
/// use confit_core::fs::MemoryFs;
/// use std::io::Cursor;
///
/// let fs = MemoryFs::new();
/// let mut input = Cursor::new("yes\n");
/// let mut output = Vec::new();
/// let seams = Seams::memory(&fs, &mut input, &mut output);
/// assert!(matches!(seams.progress, None));
/// ```
pub struct Seams<'a> {
    /// Reads plus writes backend, memory under tests.
    pub fs: &'a dyn Filesystem,
    /// Gains the confirmation answer, stdin on the host.
    pub input: &'a mut dyn BufRead,
    /// Gains previews plus prompts, stderr on the host.
    pub output: &'a mut dyn Write,
    /// Gains engine facts, holding `None` for silence.
    pub progress: Option<ProgressCallback>,
    /// Runs hook subprocesses, holding `None` for the host runner.
    pub hook_runner: Option<&'a dyn HookRunner>,
    /// Gains hook output bytes, holding `None` for no log.
    pub log_file: Option<PathBuf>,
}

impl<'a> Seams<'a> {
    /// Bundles host stdio locks with the host filesystem.
    ///
    /// # Arguments
    ///
    /// * `input` - the stdin lock under prompting.
    /// * `output` - the stderr lock under previews.
    ///
    /// # Returns
    ///
    /// Silent host seams gaining a sink through the field.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::actions::seams::Seams;
    ///
    /// let stdin = std::io::stdin();
    /// let mut input = stdin.lock();
    /// let stderr = std::io::stderr();
    /// let mut output = stderr.lock();
    /// let seams = Seams::host(&mut input, &mut output);
    /// assert!(matches!(seams.progress, None));
    /// ```
    pub fn host(input: &'a mut dyn BufRead, output: &'a mut dyn Write) -> Self {
        Self {
            fs: &HOST_FS,
            input,
            output,
            progress: None,
            hook_runner: None,
            log_file: None,
        }
    }

    /// Bundles memory fakes for tests.
    ///
    /// # Arguments
    ///
    /// * `fs` - the memory backend under reading plus writing.
    /// * `input` - the answer source under prompting.
    /// * `output` - the preview plus prompt sink.
    ///
    /// # Returns
    ///
    /// Silent memory seams gaining a sink through chaining.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::actions::seams::Seams;
    /// use confit_core::fs::MemoryFs;
    /// use std::io::Cursor;
    ///
    /// let fs = MemoryFs::new();
    /// let mut input = Cursor::new(String::new());
    /// let mut output = Vec::new();
    /// let seams = Seams::memory(&fs, &mut input, &mut output);
    /// assert!(matches!(seams.progress, None));
    /// ```
    pub fn memory(
        fs: &'a dyn Filesystem,
        input: &'a mut dyn BufRead,
        output: &'a mut dyn Write,
    ) -> Self {
        Self {
            fs,
            input,
            output,
            progress: None,
            hook_runner: None,
            log_file: None,
        }
    }

    /// Gains one engine sink while chaining.
    ///
    /// # Arguments
    ///
    /// * `sink` - the facts receiver under the run.
    ///
    /// # Returns
    ///
    /// The same seams carrying the sink.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::actions::seams::Seams;
    /// use confit_core::fs::MemoryFs;
    /// use std::io::Cursor;
    /// use std::sync::Arc;
    ///
    /// let fs = MemoryFs::new();
    /// let mut input = Cursor::new(String::new());
    /// let mut output = Vec::new();
    /// let sink = Arc::new(|_: confit_engine::ProgressEvent| {});
    /// let seams = Seams::memory(&fs, &mut input, &mut output).with_progress(sink);
    /// assert!(matches!(seams.progress, Some(_)));
    /// ```
    pub fn with_progress(mut self, sink: ProgressCallback) -> Self {
        self.progress = Some(sink);
        self
    }

    /// Prompts for the literal `yes` confirmation.
    ///
    /// # Returns
    ///
    /// True only for the literal `yes` answer.
    ///
    /// # Errors
    ///
    /// Reader plus writer failures surface as io errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_cli::actions::seams::Seams;
    /// use confit_core::fs::MemoryFs;
    /// use std::io::Cursor;
    ///
    /// let fs = MemoryFs::new();
    /// let mut input = Cursor::new("yes\n");
    /// let mut output = Vec::new();
    /// let mut seams = Seams::memory(&fs, &mut input, &mut output);
    /// assert!(matches!(seams.confirm(), Ok(true)));
    /// ```
    pub fn confirm(&mut self) -> Result<bool> {
        self.output.write_all(b"\n").map_err(Error::from)?;
        self.output
            .write_all(b"Apply these changes? Type 'yes' to continue: ")
            .map_err(Error::from)?;
        self.output.flush().map_err(Error::from)?;
        log::debug!("prompt waiting for answer");
        let mut answer = String::new();
        let reads = self.input.read_line(&mut answer).map_err(Error::from)?;
        log::debug!("prompt read {reads} bytes");
        Ok(answer.trim() == "yes")
    }

    /// Emits one hashing fact while a sink passes.
    pub fn emit_hashing(&self) {
        if let Some(sink) = self.progress.as_ref() {
            sink(ProgressEvent::Hashing);
        }
    }

    /// Emits one plan-reading fact while a sink passes.
    pub fn emit_reading_plan(&self, path: &Path) {
        if let Some(sink) = self.progress.as_ref() {
            sink(ProgressEvent::ReadingPlan {
                path: path.display().to_string(),
            });
        }
    }

    /// Emits one plan-writing fact while a sink passes.
    pub fn emit_writing_plan(&self, documents: usize) {
        if let Some(sink) = self.progress.as_ref() {
            sink(ProgressEvent::WritingPlan { documents });
        }
    }
}

/// Runs one step and logs its elapsed time at debug.
///
/// Rich log lines carrying per-step context stay inline at
/// their call sites. This covers bare steps only.
///
/// # Arguments
///
/// * `label` - the step name landing in the log line.
/// * `step` - the work under measuring.
///
/// # Returns
///
/// Whatever the step returns.
///
/// # Examples
///
/// ```rust
/// use confit_cli::actions::seams::timed;
///
/// let total = timed("sum", || 1 + 2);
/// assert!(matches!(total, 3));
/// ```
pub fn timed<T>(label: &str, step: impl FnOnce() -> T) -> T {
    let start = std::time::Instant::now();
    let out = step();
    log::debug!("{label} took {}ms", start.elapsed().as_millis());
    out
}

/// Evaluates one profile through the engine with shared flags.
pub fn evaluate_shared(
    shared: &SharedArgs,
    profile: &Path,
    progress: Option<ProgressCallback>,
) -> Result<confit_engine::Evaluation> {
    let root = resolve_root(&shared.root, Some(profile));
    let plugins = resolve_plugins(&root, &shared.plugins);
    confit_engine::evaluate(
        profile,
        confit_engine::EvalOpts {
            root,
            plugins,
            re_fetch: shared.re_fetch,
            cache_dir: None,
            fetcher: None,
            progress,
        },
    )
}

/// Logs finished documents with lifecycle status.
pub fn log_processed(built: &Bundle, previous: &Bundle) {
    for document in &built.manifest.documents {
        let status = match document.status(previous) {
            DocumentStatus::Create => "create",
            DocumentStatus::Update => "update",
            DocumentStatus::Unchanged => "unchanged",
        };
        log::debug!(
            "document processed path={} kind={} status={status}",
            document.path.as_str(),
            document.data.kind().name()
        );
    }
}
