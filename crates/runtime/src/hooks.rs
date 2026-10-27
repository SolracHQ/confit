//! Hook runs with conditions and post-checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use confit_core::error::{
    Error, ErrorCause, ErrorContext, HookReason, HookRunError, Outcome, Result, Terminal,
};
use confit_core::progress::{Event, ProgressSender};
use confit_driver as driver;
use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::hook::Hook;
use confit_model::routes::Route;

use crate::checks::{Checks, Unmet, find_executable, unmet_names};

/// One hook run decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    /// Blocked hook with its ready cause; apply pushes the argv location.
    RequiresClosed(HookRunError),
    /// Unmet need condition with its fault.
    WhenClosed(Condition, Unmet),
    /// Passing check with done work.
    ChecksPass,
    /// Runnable hook with its resolved binary.
    Run(PathBuf),
}

/// One finished hook spawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawned {
    /// The joined argv under running.
    pub argv: String,
    /// Captured stdout and stderr bytes in order.
    pub output: Vec<u8>,
    /// The process exit code.
    pub code: i32,
}

/// One hook runner with spawns and post-checks.
pub struct HookRunner {
    /// Hook events.
    pub progress: ProgressSender,
}

/// Decided hooks.
pub struct DecidedHooks {
    /// Hook verdicts in plan order.
    pub decisions: Vec<HookDecision>,
    /// Host facts behind hook conditions and post-checks.
    pub checks: Checks,
}

impl HookRunner {
    /// Decide every hook against fresh host facts.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for unresolvable binaries and commands.
    pub fn decide_all(&self, hooks: &[Hook], changed: &BTreeSet<Route>) -> Outcome<DecidedHooks> {
        let checks = Checks::current();
        let mut decisions = Vec::with_capacity(hooks.len());
        for hook in hooks {
            decisions.push(self.decide(hook, &checks, changed)?);
        }
        Ok(DecidedHooks { decisions, checks })
    }

    /// Decide one hook outcome.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for unresolvable binaries and commands.
    pub fn decide(
        &self,
        hook: &Hook,
        checks: &Checks,
        changed: &BTreeSet<Route>,
    ) -> Outcome<HookDecision> {
        let argv = Arg::join(&hook.argv);
        let scoped = scoped_checks(hook, checks).hook_failed(&argv, &HookReason::InvalidCommand)?;
        if let Some(fault) = scoped.unmet(&hook.requires, changed) {
            return Ok(HookDecision::RequiresClosed(HookRunError::Blocked {
                condition: hook.requires.describe(),
                unmet: unmet_names(&fault),
            }));
        }
        if let Some(fault) = scoped.unmet(&hook.when, changed) {
            return Ok(HookDecision::WhenClosed(hook.when.clone(), fault));
        }
        if !hook.check.is_true() && scoped.unmet(&hook.check, changed).is_none() {
            return Ok(HookDecision::ChecksPass);
        }
        let binary = match resolve_binary(hook, &scoped) {
            Ok(Some(found)) => found,
            Ok(None) => {
                let leaf = HookRunError::BinaryUnresolved;
                return Err(Error::hook_failed(
                    argv.clone(),
                    HookReason::of(&leaf),
                    ErrorContext::from(leaf),
                ));
            }
            Err(error) => {
                return Err(Error::hook_failed(
                    argv.clone(),
                    HookReason::InvalidCommand,
                    error,
                ));
            }
        };
        Ok(HookDecision::Run(binary))
    }

    /// Spawn one hook with extended PATH and a timeout.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for failed hook runs with the log chain.
    /// - [`confit_core::error::kind::ErrorKind::SpawnHook`] for failed hook runs with the log chain.
    pub fn spawn(
        &self,
        hook: &Hook,
        binary: &Path,
        position: usize,
        total: usize,
    ) -> Outcome<Spawned> {
        let argv_text = Arg::join(&hook.argv);
        let mut spawn: Vec<String> = vec![binary.display().to_string()];
        for arg in hook.argv.iter().skip(1) {
            let materialized =
                materialize_arg(arg).hook_failed(&argv_text, &HookReason::InvalidCommand)?;
            spawn.push(materialized.to_string_lossy().into_owned());
        }
        let mut path_dirs: Vec<PathBuf> = Vec::with_capacity(hook.path.len());
        for entry in &hook.path {
            let materialized =
                materialize_arg(entry).hook_failed(&argv_text, &HookReason::InvalidCommand)?;
            path_dirs.push(materialized);
        }
        let _ = self.progress.send(Event::HookRunning {
            position,
            total,
            argv: argv_text.clone(),
        });
        let outcome = match driver::process::run(&spawn, &path_dirs, hook.timeout_secs) {
            Ok(outcome) => outcome,
            Err(error) => {
                if let ErrorCause::HookRunError(leaf) = error.root_cause() {
                    let leaf = leaf.clone();
                    return Err(Error::hook_failed(
                        argv_text.clone(),
                        HookReason::of(&leaf),
                        ErrorContext::from(leaf),
                    ));
                }
                return Err(Error::spawn_hook(argv_text.clone(), error));
            }
        };
        log::debug!("hook {position} of {total} ran code={}", outcome.code);
        Ok(Spawned {
            argv: argv_text,
            output: outcome.output,
            code: outcome.code,
        })
    }

    /// Confirm one spawn code.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for failed hook runs with the log chain.
    pub fn confirm(&self, spawned: &Spawned) -> Outcome<()> {
        if spawned.code != 0 {
            let leaf = HookRunError::ExitedNonzero { code: spawned.code };
            return Err(Error::hook_failed(
                spawned.argv.clone(),
                HookReason::of(&leaf),
                ErrorContext::from(leaf),
            ));
        }
        Ok(())
    }

    /// Verify post-checks after one run.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::RunHook`] for failed hook runs with the log chain.
    pub fn verify(
        &self,
        hook: &Hook,
        checks: &Checks,
        changed: &BTreeSet<Route>,
        argv: &str,
    ) -> Outcome<()> {
        let scoped = scoped_checks(hook, checks).hook_failed(argv, &HookReason::InvalidCommand)?;
        if let Some(fault) = scoped.unmet(&hook.check, changed) {
            let leaf = HookRunError::ChecksUnmet {
                failed: unmet_names(&fault),
            };
            return Err(Error::hook_failed(
                argv.to_owned(),
                HookReason::of(&leaf),
                ErrorContext::from(leaf),
            ));
        }
        Ok(())
    }
}

/// Scope host facts to one hook with its path entries first.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn scoped_checks(hook: &Hook, checks: &Checks) -> Result<Checks> {
    let mut dirs = Vec::with_capacity(hook.path.len());
    for entry in &hook.path {
        dirs.push(materialize_arg(entry)?);
    }
    Ok(checks.extended(dirs))
}

/// Resolve one hook binary with routes expanded inline.
fn resolve_binary(hook: &Hook, scoped: &Checks) -> Result<Option<PathBuf>> {
    let Some(first) = hook.argv.first() else {
        return Ok(None);
    };
    let head = materialize_arg(first)?.to_string_lossy().into_owned();
    Ok(find_executable(&head, &scoped.path_dirs))
}

/// Materialize one hook arg into a host path.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::ParseFailed`] for unresolvable base folders.
fn materialize_arg(arg: &Arg) -> Result<PathBuf> {
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
    use confit_model::document::TextDocument;
    use confit_model::routes::{Route, RouteBase};
    use confit_store::{StoreRoots, Stores};

    use crate::write::Writer;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &std::path::Path, mode: u32) -> Document {
        Document {
            path: literal(path),
            data: Data::Text(TextDocument {
                content: "probe bytes".into(),
                mode,
                unmanaged: false,
            }),
        }
    }

    fn runner() -> HookRunner {
        let (sender, _) = crossbeam_channel::unbounded();
        HookRunner { progress: sender }
    }

    fn writer_in(dir: &std::path::Path) -> Writer {
        let (sender, _) = crossbeam_channel::unbounded();
        let roots = StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        };
        Writer {
            stores: Stores::new(roots, sender.clone()),
            progress: sender,
        }
    }

    fn write_doc(writer: &Writer, document: &Document) -> BTreeSet<Route> {
        let changed = BTreeSet::from([document.path.clone()]);
        match writer.write_documents(std::slice::from_ref(document), &changed) {
            Ok(_) => changed,
            Err(error) => panic!("documents write: {error}"),
        }
    }

    fn mise_hook(bindir: &std::path::Path) -> Hook {
        Hook {
            argv: vec![Arg::Text("mise".into()), Arg::Text("install".into())],
            path: vec![Arg::Text(bindir.display().to_string())],
            id: None,
            requires: Condition::default(),
            when: Condition::default(),
            check: Condition::default(),
            timeout_secs: 60,
        }
    }

    fn tool_hook(bindir: &std::path::Path) -> Hook {
        Hook {
            argv: vec![Arg::Text("tool".into()), Arg::Text("run".into())],
            path: vec![Arg::Text(bindir.display().to_string())],
            id: None,
            requires: Condition::default(),
            when: Condition::default(),
            check: Condition::default(),
            timeout_secs: 60,
        }
    }

    #[test]
    fn landed_mise_binary_resolves_post_write() {
        let dir = tempfile::tempdir().unwrap();
        let bindir = dir.path().join("bin");
        let binary = bindir.join("mise");
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&binary, 0o755));
        let decided = match runner().decide_all(std::slice::from_ref(&mise_hook(&bindir)), &changed)
        {
            Ok(decided) => decided,
            Err(error) => panic!("hooks decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "document-landed mise runs post-write"
        );
    }

    #[test]
    fn requires_sees_hook_path_dirs() {
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
            Err(error) => panic!("hooks decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "hook path dirs meet requires"
        );
    }

    #[test]
    fn exists_condition_sees_just_written_file() {
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
            Err(error) => panic!("hooks decide: {error}"),
        };
        assert!(
            matches!(closed.decisions[0], HookDecision::RequiresClosed(_)),
            "missing sentinel fails the condition"
        );
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&sentinel, 0o644));
        let decided = match runner().decide_all(std::slice::from_ref(&hook), &changed) {
            Ok(decided) => decided,
            Err(error) => panic!("hooks decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "written sentinel meets the condition"
        );
    }
}
