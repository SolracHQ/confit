//! Runtime
//!
//! Host facts behind condition evaluation and hook timeouts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::condition::Condition;
use crate::probe::PathProbe;

/// Default hook timeout in seconds backing the `10m` opt default.
///
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 600;

/// Host facts under condition evaluation.
///
/// Vars hold the process environment snapshot. Path dirs hold
/// the PATH entries in order. Tests build fixed values, so
/// evaluation never reads ambient state.
///
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Runtime {
    /// Holds the environment variables under reading.
    pub vars: BTreeMap<String, String>,
    /// Holds the PATH directories in search order.
    pub path_dirs: Vec<PathBuf>,
}

impl Runtime {
    /// Snapshots the host environment and PATH dirs.
    ///
    /// Non-Unicode entries drop. Missing PATH reads as empty.
    ///
    /// # Returns
    ///
    /// The runtime facts for this process.
    ///
    pub fn current() -> Self {
        let mut vars = BTreeMap::new();
        for (key, value) in std::env::vars_os() {
            if let (Ok(key), Ok(value)) = (key.into_string(), value.into_string()) {
                vars.insert(key, value);
            }
        }
        let path_dirs = std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).collect())
            .unwrap_or_default();
        Self { vars, path_dirs }
    }

    /// Evaluates one condition against runtime facts and the probe.
    ///
    /// `changed` reads membership in the changed display set.
    /// `all`, `any` and `Not` recurse.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::condition::Condition;
    /// use confit_core::probe::MemoryProbe;
    /// use confit_core::runtime::Runtime;
    /// use std::collections::{BTreeMap, BTreeSet};
    ///
    /// let rt = Runtime {
    ///     vars: BTreeMap::from([("SHELL".to_string(), "bash".to_string())]),
    ///     path_dirs: Vec::new(),
    /// };
    /// let cond = Condition::All(vec![
    ///     Condition::EnvSet { key: "SHELL".into() },
    ///     Condition::EnvEq { key: "SHELL".into(), value: "bash".into() },
    /// ]);
    /// assert_eq!(rt.evaluate(&cond, &MemoryProbe::new(), &BTreeSet::new()), true);
    /// assert_eq!(
    ///     rt.evaluate(
    ///         &Condition::EnvSet { key: "MISSING".into() },
    ///         &MemoryProbe::new(),
    ///         &BTreeSet::new()
    ///     ),
    ///     false
    /// );
    /// ```
    pub fn evaluate(
        &self,
        cond: &Condition,
        probe: &dyn PathProbe,
        changed: &BTreeSet<String>,
    ) -> bool {
        match cond {
            Condition::EnvEq { key, value } => self.vars.get(key).is_some_and(|held| held == value),
            Condition::EnvSet { key } => self.vars.get(key).is_some_and(|held| !held.is_empty()),
            Condition::InPath { name } => path_holds(name, self, probe),
            Condition::Exists { path } => probe.exists(std::path::Path::new(path)),
            Condition::Changed { path } => changed.contains(path),
            Condition::All(items) => items.iter().all(|item| self.evaluate(item, probe, changed)),
            Condition::Any(items) => items.iter().any(|item| self.evaluate(item, probe, changed)),
            Condition::Not(inner) => !self.evaluate(inner, probe, changed),
        }
    }
}

/// Reports whether one binary resolves executable on the probe.
fn path_holds(name: &str, rt: &Runtime, probe: &dyn PathProbe) -> bool {
    probe.find_executable(name, &rt.path_dirs).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::MemoryProbe;

    fn test_runtime() -> (MemoryProbe, Runtime) {
        use std::path::Path;

        let mut probe = MemoryProbe::new();
        probe.exec(Path::new("/bin/tool"));
        probe
            .file(Path::new("/bin/plain"))
            .mode(Path::new("/bin/plain"), 0o644);
        probe.file(Path::new("/opt/tool"));
        probe.file(Path::new("/home/tester/.local/bin/hook"));
        (
            probe,
            Runtime {
                vars: BTreeMap::from([
                    ("SHELL".to_string(), "bash".to_string()),
                    ("EMPTY".to_string(), String::new()),
                ]),
                path_dirs: vec![PathBuf::from("/bin"), PathBuf::from("/opt")],
            },
        )
    }

    #[test]
    fn evaluator_truth_table() {
        let (probe, rt) = test_runtime();
        let cases: Vec<(Condition, bool)> = vec![
            (
                Condition::EnvEq {
                    key: "SHELL".into(),
                    value: "bash".into(),
                },
                true,
            ),
            (
                Condition::EnvEq {
                    key: "SHELL".into(),
                    value: "zsh".into(),
                },
                false,
            ),
            (
                Condition::EnvEq {
                    key: "MISSING".into(),
                    value: "x".into(),
                },
                false,
            ),
            (
                Condition::EnvSet {
                    key: "SHELL".into(),
                },
                true,
            ),
            (
                Condition::EnvSet {
                    key: "EMPTY".into(),
                },
                false,
            ),
            (
                Condition::EnvSet {
                    key: "MISSING".into(),
                },
                false,
            ),
            (
                Condition::InPath {
                    name: "tool".into(),
                },
                true,
            ),
            (
                Condition::InPath {
                    name: "missing".into(),
                },
                false,
            ),
            (
                Condition::Exists {
                    path: "/bin/tool".into(),
                },
                true,
            ),
            (
                Condition::Exists {
                    path: "/bin/missing".into(),
                },
                false,
            ),
            (
                Condition::All(vec![
                    Condition::EnvSet {
                        key: "SHELL".into(),
                    },
                    Condition::InPath {
                        name: "tool".into(),
                    },
                ]),
                true,
            ),
            (
                Condition::All(vec![
                    Condition::EnvSet {
                        key: "SHELL".into(),
                    },
                    Condition::EnvSet {
                        key: "MISSING".into(),
                    },
                ]),
                false,
            ),
            (Condition::All(vec![]), true),
            (
                Condition::Any(vec![
                    Condition::EnvSet {
                        key: "MISSING".into(),
                    },
                    Condition::InPath {
                        name: "tool".into(),
                    },
                ]),
                true,
            ),
            (
                Condition::Any(vec![
                    Condition::EnvSet {
                        key: "MISSING".into(),
                    },
                    Condition::InPath {
                        name: "absent".into(),
                    },
                ]),
                false,
            ),
            (Condition::Any(vec![]), false),
            (
                Condition::Not(Box::new(Condition::EnvSet {
                    key: "MISSING".into(),
                })),
                true,
            ),
            (
                Condition::Not(Box::new(Condition::EnvSet {
                    key: "SHELL".into(),
                })),
                false,
            ),
            (
                Condition::Changed {
                    path: "touched".into(),
                },
                false,
            ),
            (
                Condition::All(vec![
                    Condition::Changed {
                        path: "touched".into(),
                    },
                    Condition::EnvSet {
                        key: "SHELL".into(),
                    },
                ]),
                false,
            ),
            (
                Condition::Any(vec![
                    Condition::Changed {
                        path: "touched".into(),
                    },
                    Condition::Changed {
                        path: "quiet".into(),
                    },
                ]),
                false,
            ),
            (
                Condition::Not(Box::new(Condition::Changed {
                    path: "touched".into(),
                })),
                true,
            ),
        ];
        for (cond, want) in cases {
            assert_eq!(
                rt.evaluate(&cond, &probe, &BTreeSet::new()),
                want,
                "condition {cond:?}"
            );
        }
    }

    #[test]
    fn changed_reads_membership_through_nesting() {
        let (probe, rt) = test_runtime();
        let changed: BTreeSet<String> = BTreeSet::from(["touched".to_string()]);
        let cases: Vec<(Condition, bool)> = vec![
            (
                Condition::Changed {
                    path: "touched".into(),
                },
                true,
            ),
            (
                Condition::Changed {
                    path: "quiet".into(),
                },
                false,
            ),
            (
                Condition::All(vec![
                    Condition::Changed {
                        path: "touched".into(),
                    },
                    Condition::EnvSet {
                        key: "SHELL".into(),
                    },
                ]),
                true,
            ),
            (
                Condition::All(vec![
                    Condition::Changed {
                        path: "touched".into(),
                    },
                    Condition::EnvSet {
                        key: "MISSING".into(),
                    },
                ]),
                false,
            ),
            (
                Condition::Any(vec![
                    Condition::Changed {
                        path: "quiet".into(),
                    },
                    Condition::Changed {
                        path: "touched".into(),
                    },
                ]),
                true,
            ),
            (
                Condition::Any(vec![
                    Condition::Changed {
                        path: "quiet".into(),
                    },
                    Condition::Changed {
                        path: "absent".into(),
                    },
                ]),
                false,
            ),
            (
                Condition::Not(Box::new(Condition::Changed {
                    path: "touched".into(),
                })),
                false,
            ),
            (
                Condition::Not(Box::new(Condition::Changed {
                    path: "quiet".into(),
                })),
                true,
            ),
        ];
        for (cond, want) in cases {
            assert_eq!(
                rt.evaluate(&cond, &probe, &changed),
                want,
                "condition {cond:?}"
            );
        }
    }

    #[test]
    fn in_path_skips_plain_files_for_later_executables() {
        let mut probe = MemoryProbe::new();
        probe
            .file(std::path::Path::new("/first/dup"))
            .mode(std::path::Path::new("/first/dup"), 0o644);
        probe.exec(std::path::Path::new("/second/dup"));
        let rt = Runtime {
            vars: BTreeMap::new(),
            path_dirs: vec![PathBuf::from("/first"), PathBuf::from("/second")],
        };
        assert!(rt.evaluate(
            &Condition::InPath { name: "dup".into() },
            &probe,
            &BTreeSet::new()
        ));
    }

    #[test]
    fn in_path_without_mode_reports_presence() {
        let (probe, rt) = test_runtime();
        assert!(rt.evaluate(
            &Condition::InPath {
                name: "tool".into()
            },
            &probe,
            &BTreeSet::new()
        ));
        let bare = Runtime {
            vars: BTreeMap::new(),
            path_dirs: vec![PathBuf::from("/opt")],
        };
        assert!(bare.evaluate(
            &Condition::InPath {
                name: "tool".into()
            },
            &probe,
            &BTreeSet::new()
        ));
    }

    #[test]
    fn exists_reads_probe_presence() {
        let mut probe = MemoryProbe::new();
        probe.file(std::path::Path::new("/opt/tool"));
        let rt = Runtime {
            vars: BTreeMap::new(),
            path_dirs: Vec::new(),
        };
        let found = rt.evaluate(
            &Condition::Exists {
                path: "/opt/tool".into(),
            },
            &probe,
            &BTreeSet::new(),
        );
        let missing = rt.evaluate(
            &Condition::Exists {
                path: "/opt/absent".into(),
            },
            &probe,
            &BTreeSet::new(),
        );
        assert!(found);
        assert!(!missing);
    }
}
