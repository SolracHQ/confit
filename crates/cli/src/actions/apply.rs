//! Apply run
//! Desired documents to disk writes with prompts.

use std::io::BufRead;
use std::io::Write as _;
use std::path::PathBuf;

use confit_driver::sha::Sha;
use confit_engine::Engine;
use confit_model::arg::Arg;
use confit_runtime::bundle::Bundler;
use confit_runtime::manifest::stored;
use confit_runtime::{HookDecision, HookRunner, Prepared, Writer, prepare, refresh};
use confit_store::Stores;
use confit_store::state::Picker;

use crate::error::{CliError, Result};

use crate::cli::{ApplyArgs, plan_profile};
use crate::presentation::difference::difference_lines;
use crate::presentation::hooks::describe_blocked;
use crate::presentation::summary::render_prepared;

use crate::sinks::Sinks;

/// Successful apply run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Count of documents written to disk.
    pub written: usize,
    /// Count of deleted documents and dropped tree members removed from disk.
    pub removed: usize,
}

/// Apply run from desired documents to disk writes.
pub struct ApplyRunner<'a> {
    /// Prepared manifests with differences and changed routes.
    pub prepared: Prepared,
    /// First prompt skip with difference re-prompt.
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
    /// `@name` is a named state, `%N` is history newest-first from one,
    /// `.cb` is a bundle file, folders evaluate as a project with `-p`
    /// profile choice, and everything else evaluates as a profile.
    ///
    /// # Errors
    ///
    /// - [`CliError::BadPicker`] for malformed pickers.
    /// - [`CliError::Runtime`] for unreadable states and bundles.
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
        let bundles = Bundler::new(&stores);
        let desired = if raw.starts_with('@') || raw.starts_with('%') {
            let picker: Picker = raw.parse().map_err(|_| CliError::BadPicker {
                input: raw.to_owned(),
            })?;
            stored::resolve(&stores.states(), &picker)?
        } else if positional
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
        {
            bundles.read(positional)?
        } else if positional.is_dir() {
            let path = plan_profile(positional, args.profile.as_deref());
            if args.profile.is_none() && !path.exists() {
                return Err(CliError::NoDefaultProfile {
                    project: positional.to_path_buf(),
                });
            }
            let evaluated = Engine::new(stores.clone()).evaluate(&path)?;
            let built = prepare::build(evaluated.documents, evaluated.hooks)?;
            built.persist(&stores)?;
            built
        } else {
            if args.profile.is_some() {
                return Err(CliError::ProfileWithoutProject {
                    path: positional.to_path_buf(),
                });
            }
            if !positional.exists() {
                return Err(CliError::NoProfile {
                    path: positional.to_path_buf(),
                });
            }
            let evaluated = Engine::new(stores.clone()).evaluate(positional)?;
            let built = prepare::build(evaluated.documents, evaluated.hooks)?;
            built.persist(&stores)?;
            built
        };
        let prepared = prepare(&stores, desired)?;
        Ok(Self {
            prepared,
            force: args.force,
            input,
            stores,
            sinks,
            hooks_dir,
        })
    }

    /// Apply desired documents with preview, difference prompts, and rotation.
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
    /// - [`CliError::Runtime`] for state stores.
    /// - [`CliError::Pool`] for pool prunes.
    /// - [`CliError::Resolve`] for unresolvable hooks.
    /// - [`CliError::HookRequires`] for unmet Requires conditions.
    pub fn execute(self) -> Result<ApplyReport> {
        self.sinks.print_line(render_prepared(&self.prepared));
        if !self.force && !self.sinks.confirm(self.input)? {
            return Err(CliError::Aborted);
        }
        let fresh = refresh(&self.stores, &self.prepared)?;
        if fresh != self.prepared.differences {
            for line in difference_lines(&fresh) {
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

    /// Write built documents with deleted document removal and pool rotation.
    ///
    /// # Errors
    ///
    /// - [`CliError::Runtime`] for destination writes and removals.
    /// - [`CliError::Runtime`] for state stores.
    /// - [`CliError::Pool`] for pool prunes.
    fn commit(&self) -> Result<ApplyReport> {
        let built = &self.prepared.desired;
        let previous = &self.prepared.previous;
        let changed = &self.prepared.changed;
        let writer = Writer {
            stores: self.stores.clone(),
            progress: self.sinks.progress.clone(),
        };
        let written = writer.write_documents(&built.documents, changed)?;
        let removed = writer.remove_deleted(&previous.documents, &built.documents)?;
        let removed =
            removed + writer.remove_tree_members(&previous.documents, &built.documents)?;
        stored::store(&self.stores.states(), built)?;
        let keep = stored::keep_set(&self.stores.states())?;
        self.stores.pool().prune(&keep)?;
        Ok(ApplyReport { written, removed })
    }

    /// Run built hooks after files, state, and history land.
    /// Unmet Requires conditions and hook failures abort the run.
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
                HookDecision::RequiresClosed(condition, fault) => {
                    return Err(CliError::HookRequires {
                        argv: Arg::join(&hook.argv),
                        condition: describe_blocked(condition, fault),
                    });
                }
                HookDecision::WhenClosed(_, _) | HookDecision::ChecksPass => {}
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
    let hash = Sha::hash(argv.as_bytes()).short();
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
    std::fs::create_dir_all(hooks_dir).map_err(|error| CliError::from_write_io(&log, error))?;
    let mut sink =
        std::fs::File::create(&log).map_err(|error| CliError::from_write_io(&log, error))?;
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
    sink.sync_all()
        .map_err(|error| CliError::from_write_io(&log, error))?;
    Ok(())
}
