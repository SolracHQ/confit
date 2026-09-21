//! Runtime
//!
//! Host facts behind condition evaluation and hook timeouts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::condition::Condition;
use crate::fs::Filesystem;
use crate::ids::DocPath;

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

    /// Evaluates one condition against runtime facts and the backend.
    ///
    /// `in_path` joins each dir with the name, first existing
    /// executable wins. While the backend reports a mode, the
    /// `0o111` bit decides. Otherwise plain existence decides.
    /// `exists` expands a leading tilde through the OS home
    /// folder then stats. `env_eq` and `env_set` read `vars`.
    /// `changed` reads membership in the changed-path set.
    /// `all`, `any` and `Not` recurse.
    ///
    /// # Arguments
    ///
    /// * `cond` - the condition under testing.
    /// * `fs` - the backend under stating.
    /// * `changed` - the changed document ids under reading.
    ///
    /// # Returns
    ///
    /// True while the condition holds.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::condition::Condition;
    /// use confit_core::fs::memory::MemoryFs;
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
    /// assert_eq!(rt.evaluate(&cond, &MemoryFs::new(), &BTreeSet::new()), true);
    /// assert_eq!(
    ///     rt.evaluate(
    ///         &Condition::EnvSet { key: "MISSING".into() },
    ///         &MemoryFs::new(),
    ///         &BTreeSet::new()
    ///     ),
    ///     false
    /// );
    /// ```
    pub fn evaluate(
        &self,
        cond: &Condition,
        fs: &dyn Filesystem,
        changed: &BTreeSet<DocPath>,
    ) -> bool {
        match cond {
            Condition::EnvEq { key, value } => self.vars.get(key).is_some_and(|held| held == value),
            Condition::EnvSet { key } => self.vars.get(key).is_some_and(|held| !held.is_empty()),
            Condition::InPath { name } => path_holds(name, self, fs),
            Condition::Exists { path } => fs.exists(&DocPath::new(path).expand()),
            Condition::Changed { path } => changed.contains(&DocPath::new(path)),
            Condition::All(items) => items.iter().all(|item| self.evaluate(item, fs, changed)),
            Condition::Any(items) => items.iter().any(|item| self.evaluate(item, fs, changed)),
            Condition::Not(inner) => !self.evaluate(inner, fs, changed),
        }
    }
}

/// Reports whether one binary resolves executable on the backend.
fn path_holds(name: &str, rt: &Runtime, fs: &dyn Filesystem) -> bool {
    find_binary(name, &rt.path_dirs, fs).is_some()
}

/// Finds one binary across dirs in order on the backend.
///
/// Each dir joins the name, first existing executable wins.
/// While the backend reports a mode, the `0o111` bit decides.
/// Otherwise plain existence decides.
///
/// # Arguments
///
/// * `name` - the binary name under resolving.
/// * `dirs` - the directories under searching in order.
/// * `fs` - the backend under stating.
///
/// # Returns
///
/// The joined candidate path for the first hit, else `None`.
///
pub fn find_binary(name: &str, dirs: &[PathBuf], fs: &dyn Filesystem) -> Option<PathBuf> {
    const EXEC_BIT: u32 = 0o111;
    for dir in dirs {
        let candidate = dir.join(name);
        match fs.file_mode(&candidate) {
            Some(mode) => {
                if mode & EXEC_BIT != 0 {
                    return Some(candidate);
                }
            }
            None => {
                if fs.exists(&candidate) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::memory::MemoryFs;

    fn test_runtime(fs: &MemoryFs) -> Runtime {
        use std::path::Path;

        let _ = fs.write(Path::new("/bin/tool"), b"run");
        let _ = fs.set_mode(Path::new("/bin/tool"), 0o755);
        let _ = fs.write(Path::new("/bin/plain"), b"run");
        let _ = fs.set_mode(Path::new("/bin/plain"), 0o644);
        let _ = fs.write(Path::new("/opt/tool"), b"run");
        let _ = fs.write(Path::new("/home/tester/.local/bin/hook"), b"run");
        Runtime {
            vars: BTreeMap::from([
                ("SHELL".to_string(), "bash".to_string()),
                ("EMPTY".to_string(), String::new()),
            ]),
            path_dirs: vec![PathBuf::from("/bin"), PathBuf::from("/opt")],
        }
    }

    #[test]
    fn evaluator_truth_table() {
        let fs = MemoryFs::new();
        let rt = test_runtime(&fs);
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
                rt.evaluate(&cond, &fs, &BTreeSet::new()),
                want,
                "condition {cond:?}"
            );
        }
    }

    #[test]
    fn changed_reads_membership_through_nesting() {
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        let rt = test_runtime(&fs);
        let changed: BTreeSet<DocPath> = BTreeSet::from([DocPath::new("touched")]);
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
                rt.evaluate(&cond, &fs, &changed),
                want,
                "condition {cond:?}"
            );
        }
    }

    #[test]
    fn in_path_skips_plain_files_for_later_executables() {
        let fs = MemoryFs::new();
        let _ = fs.write(std::path::Path::new("/first/dup"), b"run");
        let _ = fs.set_mode(std::path::Path::new("/first/dup"), 0o644);
        let _ = fs.write(std::path::Path::new("/second/dup"), b"run");
        let _ = fs.set_mode(std::path::Path::new("/second/dup"), 0o755);
        let rt = Runtime {
            vars: BTreeMap::new(),
            path_dirs: vec![PathBuf::from("/first"), PathBuf::from("/second")],
        };
        assert!(rt.evaluate(
            &Condition::InPath { name: "dup".into() },
            &fs,
            &BTreeSet::new()
        ));
    }

    #[test]
    fn in_path_without_mode_reports_presence() {
        let fs = MemoryFs::new();
        let rt = test_runtime(&fs);
        assert!(rt.evaluate(
            &Condition::InPath {
                name: "tool".into()
            },
            &fs,
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
            &fs,
            &BTreeSet::new()
        ));
    }

    #[test]
    fn exists_expands_tilde_through_home() {
        let previous = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", "/tmp/confit-runtime-fixture");
        }
        let fs = MemoryFs::new();
        let _ = fs.write(
            std::path::Path::new("/tmp/confit-runtime-fixture/.local/bin/hook"),
            b"run",
        );
        let rt = Runtime {
            vars: BTreeMap::new(),
            path_dirs: Vec::new(),
        };
        let found = rt.evaluate(
            &Condition::Exists {
                path: "~/.local/bin/hook".into(),
            },
            &fs,
            &BTreeSet::new(),
        );
        let missing = rt.evaluate(
            &Condition::Exists {
                path: "~/.local/bin/absent".into(),
            },
            &fs,
            &BTreeSet::new(),
        );
        match previous {
            Some(value) => unsafe {
                std::env::set_var("HOME", value);
            },
            None => unsafe {
                std::env::remove_var("HOME");
            },
        }
        assert!(found);
        assert!(!missing);
    }
}
