//! Apply run
//! Desired documents to disk writes with prompts.

use std::io::BufRead;
use std::io::Write as _;
use std::path::PathBuf;

use confit_driver as driver;
use confit_model::arg::Arg;
use confit_runtime::{HookDecision, HookRunner, Prepared, Writer};
use confit_store::Stores;

use crate::error::{CliError, Result};

use crate::cli::{ApplyArgs, plan_profile};
use crate::presentation::drift::drift_lines;
use crate::presentation::hooks::describe_condition;
use crate::presentation::summary::render_prepared;

use crate::sinks::Sinks;

/// Successful apply run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Count of documents written to disk.
    pub written: usize,
    /// Count of recorded orphans and dropped tree members removed from disk.
    pub removed: usize,
}

/// Apply run from desired documents to disk writes.
pub struct ApplyRunner<'a> {
    /// Prepared manifests with drift and changed routes.
    pub prepared: Prepared,
    /// First prompt skip with drift re-prompt.
    pub force: bool,
    /// Confirmation answer reader, stdin on the host.
    pub input: &'a mut dyn BufRead,
    /// Write capabilities for the run.
    pub stores: Stores,
    /// Output senders for the run.
    pub sinks: Sinks,
    /// Hook output folder holding one file per hook.
    pub hooks_dir: PathBuf,
}

impl<'a> ApplyRunner<'a> {
    /// Resolve desired documents from flags with explicit run fields.
    ///
    /// `@name` is a named slot, `%N` is history newest-first from one,
    /// `.cb` is a bundle file, folders evaluate as a project with `-p`
    /// profile choice, and everything else evaluates as a profile.
    ///
    /// # Errors
    ///
    /// - [`CliError::Slot`] for slot reads.
    /// - [`CliError::Bundle`] for bundle reads.
    /// - [`CliError::Engine`] for evaluation failures.
    /// - [`CliError::NoProfile`] for absent profiles.
    /// - [`CliError::NoDefaultProfile`] for absent defaults.
    /// - [`CliError::ProfileWithoutProject`] for `-p` with file sources.
    pub fn from_args(
        args: &ApplyArgs,
        input: &'a mut dyn BufRead,
        stores: Stores,
        sinks: Sinks,
        hooks_dir: PathBuf,
    ) -> Result<Self> {
        let positional = args.source.as_path();
        let raw = positional.to_str().unwrap_or("");
        let desired = if raw.starts_with('@') || raw.starts_with('%') {
            let (manifest, _) = stores.slots().resolve(Some(raw))?;
            manifest
        } else if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            stores.bundles().read(positional)?
        } else if driver::fs::is_dir(positional) {
            let path = plan_profile(positional, args.profile.as_deref());
            if args.profile.is_none() && driver::fs::is_missing(&path) {
                return Err(CliError::NoDefaultProfile {
                    project: positional.to_path_buf(),
                });
            }
            confit_engine::Engine::new(stores.clone()).evaluate(&path)?
        } else {
            if args.profile.is_some() {
                return Err(CliError::ProfileWithoutProject {
                    path: positional.to_path_buf(),
                });
            }
            if driver::fs::is_missing(positional) {
                return Err(CliError::NoProfile {
                    path: positional.to_path_buf(),
                });
            }
            confit_engine::Engine::new(stores.clone()).evaluate(positional)?
        };
        let prepared = confit_runtime::prepare(&stores, desired)?;
        Ok(Self {
            prepared,
            force: args.force,
            input,
            stores,
            sinks,
            hooks_dir,
        })
    }

    /// Apply desired documents with preview, drift prompts, and rotation.
    ///
    /// Success with the state file and one stored manifest with rotation.
    /// Report with the write counts.
    ///
    /// - Literal `yes` for progress, anything else for an abort without writes.
    ///
    /// # Errors
    ///
    /// - [`CliError::Aborted`] for non-`yes` answers.
    /// - [`CliError::Unknown`] for prompt failures.
    /// - [`CliError::Runtime`] for destination writes and removals.
    /// - [`CliError::Slot`] for state stores.
    /// - [`CliError::Blob`] for blob resolves with persists and prunes.
    /// - [`CliError::Resolve`] for unresolvable hooks.
    /// - [`CliError::HookRequires`] for closed Requires gates.
    pub fn execute(self) -> Result<ApplyReport> {
        self.sinks.print_line(render_prepared(&self.prepared));
        if !self.force && !self.sinks.confirm(self.input)? {
            return Err(CliError::Aborted);
        }
        let fresh = confit_runtime::refresh(&self.stores, &self.prepared);
        if fresh != self.prepared.drifts {
            for line in drift_lines(&fresh) {
                self.sinks.print_line(line);
            }
            if !self.sinks.confirm(self.input)? {
                return Err(CliError::Aborted);
            }
        }
        let report = self.commit()?;
        self.run_hooks()?;
        Ok(report)
    }

    /// Write built documents with orphan removal and blob rotation.
    ///
    /// # Errors
    ///
    /// - [`CliError::Runtime`] for destination writes and removals.
    /// - [`CliError::Slot`] for state stores.
    /// - [`CliError::Blob`] for blob resolves with persists and prunes.
    fn commit(&self) -> Result<ApplyReport> {
        let built = &self.prepared.desired;
        let previous = &self.prepared.previous;
        let changed = &self.prepared.changed;
        let writer = Writer {
            stores: self.stores.clone(),
            progress: self.sinks.progress.clone(),
        };
        let written = writer.write_documents(&built.documents, changed)?;
        let removed = writer.remove_orphans(&previous.documents, &built.documents)?;
        let removed =
            removed + writer.remove_tree_members(&previous.documents, &built.documents)?;
        let _ = self.stores.slots().store(built)?;
        let mut handles = Vec::new();
        for document in &built.documents {
            for blob in document.data.blob_refs() {
                handles.push(self.stores.blobs().resolve(blob)?);
            }
        }
        self.stores.blobs().persist(&handles)?;
        self.stores.blobs().prune()?;
        Ok(ApplyReport { written, removed })
    }

    /// Run built hooks after files, state, and history land, aborting on
    /// closed Requires gates and hook failures.
    fn run_hooks(&self) -> Result<()> {
        let runner = HookRunner {
            progress: self.sinks.progress.clone(),
        };
        let prepared = &self.prepared;
        let decided = runner.decide_all(&prepared.desired.hooks, &prepared.changed)?;
        let total = prepared.desired.hooks.len();
        for (index, hook) in prepared.desired.hooks.iter().enumerate() {
            let position = index + 1;
            match &decided.decisions[index] {
                HookDecision::RequiresClosed(gate) => {
                    return Err(CliError::HookRequires {
                        argv: Arg::join(&hook.argv),
                        condition: describe_condition(gate),
                    });
                }
                HookDecision::WhenClosed(_) | HookDecision::ChecksPass => {}
                HookDecision::Run(binary) => {
                    let line = format!("hook {position} of {total}: {}", Arg::join(&hook.argv));
                    self.sinks.print_line(line.clone());
                    let spawned = runner.spawn(hook, binary, position, total)?;
                    let file_name = hook_file_name(&spawned.argv);
                    write_hook_log(&self.hooks_dir, &file_name, &line, &spawned.output)?;
                    log::info!(
                        "{line} output in {}",
                        self.hooks_dir.join(&file_name).display()
                    );
                    runner.confirm(&spawned)?;
                    runner.verify(hook, &decided.checks, &prepared.changed, &spawned.argv)?;
                }
            }
        }
        Ok(())
    }
}

/// Build one hook log file name from its slug and hash.
fn hook_file_name(argv: &str) -> String {
    let mut slug = argv
        .split(|cell: char| !cell.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        slug = "hook".to_owned();
    }
    if slug.len() > 40 {
        slug.truncate(40);
    }
    let hash = confit_model::sha::Sha::hash(argv.as_bytes()).short();
    format!("{slug}-{hash}.log")
}

/// Write one hook header and captured bytes into its own log file, with
/// parent folders created as needed.
///
/// # Errors
///
/// - [`CliError::Write`] with [`CliError::WriteUnknown`]
///   for failed log writes.
fn write_hook_log(
    hooks_dir: &std::path::Path,
    file_name: &str,
    header: &str,
    output: &[u8],
) -> Result<()> {
    let log = hooks_dir.join(file_name);
    driver::fs::create_dir_all(hooks_dir).map_err(|error| CliError::from_write_io(&log, error))?;
    let mut sink =
        driver::fs::create(&log).map_err(|error| CliError::from_write_io(&log, error))?;
    sink.write_all(header.as_bytes())
        .map_err(|error| CliError::from_write_io(&log, error))?;
    sink.write_all(b"\n")
        .map_err(|error| CliError::from_write_io(&log, error))?;
    sink.write_all(output)
        .map_err(|error| CliError::from_write_io(&log, error))?;
    if !output.ends_with(b"\n") {
        sink.write_all(b"\n")
            .map_err(|error| CliError::from_write_io(&log, error))?;
    }
    sink.sync()
        .map_err(|error| CliError::from_write_io(&log, error))?;
    Ok(())
}
