//! Hook runs with conditions and post-checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use confit_core::error::kind::ErrorKind;
use confit_core::error::{
    ContextualCause, ContextualResult, Error, ErrorCause, HookRunError, Terminal, TerminalResult,
    WithContext, contextual, error,
};
use confit_core::sha::Sha;
use confit_driver as driver;
use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::hook::Hook;
use confit_model::routes::Route;
use confit_ui::Event;
use confit_ui::ToUi;
use crossbeam_channel::Sender;

use crate::checks::{Checks, Unmet, find_executable};

/// The decision for one hook.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    /// A hook closed by its bare run failure.
    RequiresClosed(HookRunError),
    /// An unmet `when` condition and its fault.
    WhenClosed(Condition, Unmet),
    /// A hook whose check passes and needs no run.
    ChecksPass,
    /// A hook that runs, with its resolved binary.
    Run(PathBuf),
}

/// One finished hook process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawned {
    /// The joined argv of the run.
    pub argv: String,
    /// The process exit code.
    pub code: i32,
    /// The hook log holding the streamed output.
    pub log: PathBuf,
}

/// The runner for hook spawns and post-checks.
pub struct HookRunner {
    /// The progress channel for hook events.
    pub to_ui: Sender<ToUi>,
}

/// The decision for every hook.
pub struct DecidedHooks {
    /// The decisions in plan order.
    pub decisions: Vec<HookDecision>,
    /// The host facts that hook conditions and post-checks test.
    pub checks: Checks,
}

impl HookRunner {
    /// Decides every hook and returns the decisions with the facts in use.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a binary or command fails to resolve.
    pub fn decide_all(
        &self,
        hooks: &[Hook],
        changed: &BTreeSet<Route>,
    ) -> TerminalResult<DecidedHooks> {
        let checks = Checks::current();
        let mut decisions = Vec::with_capacity(hooks.len());
        for hook in hooks {
            decisions.push(self.decide(hook, &checks, changed)?);
        }
        Ok(DecidedHooks { decisions, checks })
    }

    /// Decides one hook and returns its decision.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a binary or command fails to resolve.
    pub fn decide(
        &self,
        hook: &Hook,
        checks: &Checks,
        changed: &BTreeSet<Route>,
    ) -> TerminalResult<HookDecision> {
        let scoped = scoped_checks(hook, checks)
            .with_argv(Arg::join(&hook.argv))
            .terminal(ErrorKind::RunHook)?;
        if scoped.unmet(&hook.requires, changed).is_some() {
            return Ok(HookDecision::RequiresClosed(HookRunError::Blocked));
        }
        if let Some(fault) = scoped.unmet(&hook.when, changed) {
            return Ok(HookDecision::WhenClosed(hook.when.clone(), fault));
        }
        if !hook.check.is_true() && scoped.unmet(&hook.check, changed).is_none() {
            return Ok(HookDecision::ChecksPass);
        }
        let binary = match resolve_binary(hook, &scoped)
            .with_argv(Arg::join(&hook.argv))
            .terminal(ErrorKind::RunHook)?
        {
            Some(found) => found,
            None => {
                return error(
                    ErrorKind::RunHook,
                    contextual(HookRunError::BinaryUnresolved).with_argv(Arg::join(&hook.argv)),
                );
            }
        };
        Ok(HookDecision::Run(binary))
    }

    /// Spawns one hook with the hook PATH entries first and the hook timeout.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when an arg fails to materialize or a hook run fails.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] when the process fails to spawn.
    pub fn spawn(
        &self,
        hook: &Hook,
        binary: &Path,
        position: usize,
        total: usize,
        hooks_dir: &Path,
    ) -> TerminalResult<Spawned> {
        let argv_text = Arg::join(&hook.argv);
        let mut spawn: Vec<String> = vec![binary.display().to_string()];
        for arg in hook.argv.iter().skip(1) {
            let materialized = materialize_arg(arg)
                .with_argv(&argv_text)
                .terminal(ErrorKind::RunHook)?;
            spawn.push(materialized.to_string_lossy().into_owned());
        }
        let mut path_dirs: Vec<PathBuf> = Vec::with_capacity(hook.path.len());
        for entry in &hook.path {
            let materialized = materialize_arg(entry)
                .with_argv(&argv_text)
                .terminal(ErrorKind::RunHook)?;
            path_dirs.push(materialized);
        }
        let _ = self.to_ui.send(ToUi::Progress(Event::Started {
            id: argv_text.clone(),
            label: format!("hook {position} of {total}: {argv_text}"),
        }));
        let log = hooks_dir.join(hook_file_name(hook.id.as_str()));
        let header = format!("hook {position} of {total}: {argv_text}");
        let file = open_hook_log(&log, &header)?;
        let outcome = driver::process::run(&spawn, &path_dirs, hook.timeout_secs.secs(), &file)
            .map_err(|contextual| {
                let kind = match contextual.cause() {
                    ErrorCause::HookRunError(_) => ErrorKind::RunHook,
                    _ => ErrorKind::SpawnHook,
                };
                Error::new(kind, contextual.with_argv(&argv_text).with_fs_path(&log))
            })?;
        log::debug!("hook {position} of {total} ran code={}", outcome.code);
        let _ = self.to_ui.send(ToUi::Progress(Event::Finished {
            id: argv_text.clone(),
            label: format!("hook {position} of {total} done: {argv_text}"),
        }));
        Ok(Spawned {
            argv: argv_text,
            code: outcome.code,
            log,
        })
    }

    /// Confirms that one spawn exits with code zero.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when the exit code is nonzero.
    pub fn confirm(&self, spawned: &Spawned) -> TerminalResult<()> {
        if spawned.code != 0 {
            return error(
                ErrorKind::RunHook,
                contextual(HookRunError::ExitedNonzero { code: spawned.code })
                    .with_argv(&spawned.argv),
            );
        }
        Ok(())
    }

    /// Checks the hook conditions after one run.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] when a post-check is unmet.
    pub fn verify(
        &self,
        hook: &Hook,
        checks: &Checks,
        changed: &BTreeSet<Route>,
        argv: &str,
    ) -> TerminalResult<()> {
        let scoped = scoped_checks(hook, checks)
            .with_argv(argv.to_owned())
            .terminal(ErrorKind::RunHook)?;
        if scoped.unmet(&hook.check, changed).is_some() {
            return error(
                ErrorKind::RunHook,
                contextual(HookRunError::ChecksUnmet).with_argv(argv.to_owned()),
            );
        }
        Ok(())
    }
}

/// Creates one hook log file and writes the header line.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::WriteHookLog`] when a log write fails.
fn open_hook_log(path: &Path, header: &str) -> TerminalResult<std::fs::File> {
    use std::io::Write as _;

    if let Some(parent) = path.parent() {
        driver::fs::create_parent(parent).terminal(ErrorKind::WriteHookLog)?;
    }
    let mut file = std::fs::File::create(path)
        .map_err(ContextualCause::from)
        .with_fs_path(path)
        .terminal(ErrorKind::WriteHookLog)?;
    file.write_all(header.as_bytes())
        .map_err(ContextualCause::from)
        .with_fs_path(path)
        .terminal(ErrorKind::WriteHookLog)?;
    file.write_all(b"\n")
        .map_err(ContextualCause::from)
        .with_fs_path(path)
        .terminal(ErrorKind::WriteHookLog)?;
    Ok(file)
}

/// Builds one log file name from the hook id and a short hash of it.
fn hook_file_name(id: &str) -> String {
    let mut slug = id
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
    let hash = Sha::hash(id.as_bytes()).short();
    format!("{slug}-{hash}.log")
}

/// Extends the host facts with the hook PATH entries in front.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn scoped_checks(hook: &Hook, checks: &Checks) -> ContextualResult<Checks> {
    let mut dirs = Vec::with_capacity(hook.path.len());
    for entry in &hook.path {
        dirs.push(materialize_arg(entry)?);
    }
    Ok(checks.extended(dirs))
}

/// Resolves one hook binary and expands its routes inline.
fn resolve_binary(hook: &Hook, scoped: &Checks) -> ContextualResult<Option<PathBuf>> {
    let Some(first) = hook.argv.first() else {
        return Ok(None);
    };
    let head = materialize_arg(first)?.to_string_lossy().into_owned();
    Ok(find_executable(&head, &scoped.path_dirs))
}

/// Expands one hook arg into a host path.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] when a base folder is absent.
fn materialize_arg(arg: &Arg) -> ContextualResult<PathBuf> {
    match arg {
        Arg::Text(text) => Ok(PathBuf::from(text)),
        Arg::Route(route) => route.expand(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use confit_model::document::Data;
    use confit_model::document::Document;
    use confit_model::document::{FileMode, TextDocument};
    use confit_model::hook::HookTimeout;
    use confit_model::id::Id;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::{StoreRoots, Stores};

    use crate::write::Writer;

    fn literal(path: &Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &Path, mode: u32) -> Document {
        Document {
            path: literal(path),
            data: Data::Text(TextDocument {
                content: "probe bytes".into(),
                mode: FileMode::new(mode),
                unmanaged: false,
            }),
        }
    }

    fn runner() -> HookRunner {
        let (sender, _) = crossbeam_channel::unbounded();
        HookRunner { to_ui: sender }
    }

    fn writer_in(dir: &Path) -> Writer {
        let (sender, _) = crossbeam_channel::unbounded();
        let roots = StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        };
        Writer {
            stores: Stores::new(roots, sender.clone()),
            to_ui: sender,
        }
    }

    fn write_doc(writer: &Writer, document: &Document) -> BTreeSet<Route> {
        let changed = BTreeSet::from([document.path.clone()]);
        match writer.write_documents(std::slice::from_ref(document), &changed) {
            Ok(_) => changed,
            Err(error) => panic!("the documents fail to write: {error}"),
        }
    }

    fn mise_hook(bindir: &Path) -> Hook {
        Hook {
            argv: vec![Arg::Text("mise".into()), Arg::Text("install".into())],
            path: vec![Arg::Text(bindir.display().to_string())],
            id: Id::of("mise-install"),
            requires: Condition::default(),
            when: Condition::default(),
            check: Condition::default(),
            timeout_secs: HookTimeout::new(60),
        }
    }

    fn tool_hook(bindir: &Path) -> Hook {
        Hook {
            argv: vec![Arg::Text("tool".into()), Arg::Text("run".into())],
            path: vec![Arg::Text(bindir.display().to_string())],
            id: Id::of("tool-run"),
            requires: Condition::default(),
            when: Condition::default(),
            check: Condition::default(),
            timeout_secs: HookTimeout::new(60),
        }
    }

    #[test]
    fn written_mise_binary_resolves_after_the_write() {
        let dir = tempfile::tempdir().unwrap();
        let bindir = dir.path().join("bin");
        let binary = bindir.join("mise");
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&binary, 0o755));
        let decided = match runner().decide_all(std::slice::from_ref(&mise_hook(&bindir)), &changed)
        {
            Ok(decided) => decided,
            Err(error) => panic!("the hooks fail to decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "the written mise binary runs after the write"
        );
    }

    #[test]
    fn requires_condition_sees_the_hook_path_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let bindir = dir.path().join("bin");
        let binary = bindir.join("mise");
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&binary, 0o755));
        let mut hook = mise_hook(&bindir);
        hook.requires = Condition::InPath {
            name: "mise".into(),
        };
        let decided = match runner().decide_all(std::slice::from_ref(&hook), &changed) {
            Ok(decided) => decided,
            Err(error) => panic!("the hooks fail to decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "the hook path dirs meet the requires condition"
        );
    }

    #[test]
    fn exists_condition_sees_a_written_file() {
        let dir = tempfile::tempdir().unwrap();
        let bindir = dir.path().join("bin");
        let binary = bindir.join("tool");
        std::fs::create_dir_all(&bindir).unwrap();
        std::fs::write(&binary, b"run").unwrap();
        driver::fs::set_mode(&binary, Some(0o755)).unwrap();
        let sentinel = dir.path().join("sentinel.txt");
        let mut hook = tool_hook(&bindir);
        hook.requires = Condition::Exists {
            route: literal(&sentinel),
        };
        let closed = match runner().decide_all(std::slice::from_ref(&hook), &BTreeSet::new()) {
            Ok(decided) => decided,
            Err(error) => panic!("the hooks fail to decide: {error}"),
        };
        assert!(
            matches!(closed.decisions[0], HookDecision::RequiresClosed(_)),
            "the requires condition rejects an absent sentinel"
        );
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&sentinel, 0o644));
        let decided = match runner().decide_all(std::slice::from_ref(&hook), &changed) {
            Ok(decided) => decided,
            Err(error) => panic!("the hooks fail to decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "the written sentinel meets the condition"
        );
    }
}
