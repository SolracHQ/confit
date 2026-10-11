//! Hook runs with gates and post-checks.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use confit_driver as driver;
use confit_driver::progress::{Event, ProgressSender};
use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::hook::Hook;
use confit_model::routes::Route;

use crate::checks::{Checks, find_executable};
use crate::error::{Result, RuntimeError};

/// One hook run decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    /// Closed capability gate with its condition.
    RequiresClosed(Condition),
    /// Closed need gate with its condition.
    WhenClosed(Condition),
    /// Passing checks with done work.
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

/// Hook gates with spawns and post-checks.
pub struct HookRunner {
    /// Hook events.
    pub progress: ProgressSender,
}

/// Decided hooks with the facts behind the verdicts.
pub struct DecidedHooks {
    /// Hook verdicts in plan order.
    pub decisions: Vec<HookDecision>,
    /// Host facts behind hook gates and post-checks.
    pub checks: Checks,
}

impl HookRunner {
    /// Decide every hook against fresh host facts.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::HookResolve`] for unresolvable binaries.
    pub fn decide_all(&self, hooks: &[Hook], changed: &BTreeSet<Route>) -> Result<DecidedHooks> {
        let checks = Checks::current();
        let mut decisions = Vec::with_capacity(hooks.len());
        for hook in hooks {
            decisions.push(self.decide(hook, &checks, changed)?);
        }
        Ok(DecidedHooks { decisions, checks })
    }

    /// Decide one hook outcome.
    ///
    /// - Requires, when, and checks gates run before resolving.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::HookResolve`] for unresolvable binaries.
    pub fn decide(
        &self,
        hook: &Hook,
        checks: &Checks,
        changed: &BTreeSet<Route>,
    ) -> Result<HookDecision> {
        if let Some(gate) = hook.requires.as_ref()
            && !checks.check(gate, changed)
        {
            return Ok(HookDecision::RequiresClosed(gate.clone()));
        }
        if let Some(gate) = hook.when.as_ref()
            && !checks.check(gate, changed)
        {
            return Ok(HookDecision::WhenClosed(gate.clone()));
        }
        if !hook.checks.is_empty() && hook.checks.iter().all(|check| checks.check(check, changed)) {
            return Ok(HookDecision::ChecksPass);
        }
        let Some(binary) = resolve_binary(hook, checks) else {
            return Err(RuntimeError::HookResolve {
                argv: Arg::join(&hook.argv),
                head: hook.argv.first().map(Arg::display).unwrap_or_default(),
            });
        };
        Ok(HookDecision::Run(binary))
    }

    /// Spawn one hook with extended PATH and a timeout.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::EmptyArgv`] for empty argv.
    /// - [`RuntimeError::Spawn`] for spawn failures.
    /// - [`RuntimeError::Wait`] for wait failures.
    /// - [`RuntimeError::Timeout`] for timed-out runs.
    /// - [`RuntimeError::Output`] for output join failures.
    pub fn spawn(
        &self,
        hook: &Hook,
        binary: &Path,
        position: usize,
        total: usize,
    ) -> Result<Spawned> {
        let argv_text = Arg::join(&hook.argv);
        let mut spawn: Vec<String> = vec![binary.display().to_string()];
        for slot in hook.argv.iter().skip(1) {
            spawn.push(slot.materialize().to_string_lossy().into_owned());
        }
        let mut path_dirs: Vec<PathBuf> = Vec::with_capacity(hook.path.len());
        for slot in &hook.path {
            path_dirs.push(slot.materialize());
        }
        let _ = self.progress.send(Event::HookRunning {
            position,
            total,
            argv: argv_text.clone(),
        });
        let outcome = driver::process::run(&spawn, &path_dirs, hook.timeout_secs)
            .map_err(|error| map_process(error, &argv_text))?;
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
    /// - [`RuntimeError::HookFailed`] for nonzero codes.
    pub fn confirm(&self, spawned: &Spawned) -> Result<()> {
        if spawned.code != 0 {
            return Err(RuntimeError::HookFailed {
                argv: spawned.argv.clone(),
                code: spawned.code,
            });
        }
        Ok(())
    }

    /// Verify post-checks after one run.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::HookChecks`] for unmet post-checks.
    pub fn verify(
        &self,
        hook: &Hook,
        checks: &Checks,
        changed: &BTreeSet<Route>,
        argv: &str,
    ) -> Result<()> {
        if hook.checks.is_empty() {
            return Ok(());
        }
        let failed: Vec<Condition> = hook
            .checks
            .iter()
            .filter(|check| !checks.check(check, changed))
            .cloned()
            .collect();
        if failed.is_empty() {
            return Ok(());
        }
        Err(RuntimeError::HookChecks {
            argv: argv.to_owned(),
            failed,
        })
    }
}

/// Resolve one hook binary with routes expanded inline.
///
/// Route slots map against host folders, text unchanged.
///
/// - Hook path entries precede runtime dirs.
/// - The first existing executable wins.
fn resolve_binary(hook: &Hook, checks: &Checks) -> Option<PathBuf> {
    let head = hook
        .argv
        .first()
        .map(|slot| slot.materialize().to_string_lossy().into_owned())?;
    let mut dirs: Vec<PathBuf> = hook.path.iter().map(Arg::materialize).collect();
    dirs.extend(checks.path_dirs.iter().cloned());
    find_executable(&head, &dirs)
}

/// Map one driver process failure onto runtime language with argv.
fn map_process(error: driver::process::ProcessError, argv: &str) -> RuntimeError {
    match error {
        driver::process::ProcessError::Empty => RuntimeError::EmptyArgv,
        driver::process::ProcessError::Spawn { fault } => RuntimeError::Spawn {
            argv: argv.to_owned(),
            fault,
        },
        driver::process::ProcessError::Wait { fault } => RuntimeError::Wait {
            argv: argv.to_owned(),
            fault,
        },
        driver::process::ProcessError::Timeout { timeout_secs } => RuntimeError::Timeout {
            argv: argv.to_owned(),
            timeout_secs,
        },
        driver::process::ProcessError::Output => RuntimeError::Output {
            argv: argv.to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    use confit_driver as driver;
    use confit_driver::fs::TestGuard;
    use confit_model::document::{Data, Document};
    use confit_model::routes::{Route, RouteBase};
    use confit_store::{StoreRoots, Stores};

    use crate::write::Writer;

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &std::path::Path, mode: Option<u32>) -> Document {
        Document::new(
            literal(path),
            Data::Text {
                content: "probe bytes".into(),
                mode,
                unmanaged: false,
            },
        )
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
        let changed = BTreeSet::from([document.destination.clone()]);
        match writer.write_documents(std::slice::from_ref(document), &changed) {
            Ok(_) => changed,
            Err(error) => panic!("documents write: {error}"),
        }
    }

    fn mise_hook(bindir: &std::path::Path) -> Hook {
        Hook {
            argv: vec![Arg::Text("mise".into()), Arg::Text("install".into())],
            path: vec![Arg::Text(bindir.display().to_string())],
            requires: None,
            when: None,
            checks: Vec::new(),
            timeout_secs: 60,
        }
    }

    fn tool_hook(bindir: &std::path::Path) -> Hook {
        Hook {
            argv: vec![Arg::Text("tool".into()), Arg::Text("run".into())],
            path: vec![Arg::Text(bindir.display().to_string())],
            requires: None,
            when: None,
            checks: Vec::new(),
            timeout_secs: 60,
        }
    }

    #[test]
    fn landed_mise_binary_resolves_post_write() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let bindir = dir.path().join("bin");
        let binary = bindir.join("mise");
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&binary, Some(0o755)));
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
    fn exists_gate_sees_just_written_file() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let bindir = dir.path().join("bin");
        let binary = bindir.join("tool");
        driver::fs::create_dir_all(&bindir).unwrap();
        driver::fs::write(&binary, b"run").unwrap();
        driver::fs::set_mode(&binary, 0o755).unwrap();
        let sentinel = dir.path().join("sentinel.txt");
        let mut hook = tool_hook(&bindir);
        hook.requires = Some(Condition::Exists {
            route: literal(&sentinel),
        });
        let closed = match runner().decide_all(std::slice::from_ref(&hook), &BTreeSet::new()) {
            Ok(decided) => decided,
            Err(error) => panic!("hooks decide: {error}"),
        };
        assert!(
            matches!(closed.decisions[0], HookDecision::RequiresClosed(_)),
            "missing sentinel closes the gate"
        );
        let writer = writer_in(dir.path());
        let changed = write_doc(&writer, &text_doc(&sentinel, None));
        let decided = match runner().decide_all(std::slice::from_ref(&hook), &changed) {
            Ok(decided) => decided,
            Err(error) => panic!("hooks decide: {error}"),
        };
        assert_eq!(
            decided.decisions,
            vec![HookDecision::Run(binary)],
            "written sentinel opens the gate"
        );
    }
}
