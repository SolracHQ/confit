//! Runtime
//!
//! Host facts behind condition evaluation plus hook timeouts.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::document::Condition;
use crate::fs::Filesystem;
use crate::ids::DocPath;

/// Default hook timeout in seconds backing the `10m` opt default.
///
/// # Examples
///
/// ```text
/// use confit_core::runtime::DEFAULT_HOOK_TIMEOUT_SECS;
///
/// assert!(matches!(DEFAULT_HOOK_TIMEOUT_SECS, 600));
/// ```
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 600;

/// Host facts under condition evaluation.
///
/// Vars hold the process environment snapshot. Path dirs hold
/// the PATH entries in order. Tests build fixed values, so
/// evaluation never reads ambient state.
///
/// # Examples
///
/// ```text
/// use confit_core::runtime::Runtime;
///
/// let rt = Runtime { vars: Default::default(), path_dirs: Vec::new() };
/// assert!(matches!(rt.vars.is_empty(), true));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Runtime {
    /// Holds the environment variables under reading.
    pub vars: BTreeMap<String, String>,
    /// Holds the PATH directories in search order.
    pub path_dirs: Vec<PathBuf>,
}

impl Runtime {
    /// Snapshots the host environment plus PATH dirs.
    ///
    /// Non-Unicode entries drop. Missing PATH reads as empty.
    ///
    /// # Returns
    ///
    /// The runtime facts for this process.
    ///
    /// # Examples
    ///
    /// ```text
    /// use confit_core::runtime::Runtime;
    ///
    /// let rt = Runtime::current();
    /// assert!(matches!(rt.vars.is_empty(), true | false));
    /// ```
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
}

/// Evaluates one condition against runtime facts plus the backend.
///
/// `in_path` joins each dir with the name, first existing
/// executable wins. While the backend reports a mode, the
/// `0o111` bit decides. Otherwise plain existence decides.
/// `exists` expands a leading tilde through the OS home
/// folder then stats. `env_eq` plus `env_set` read `vars`.
/// `all` plus `any` plus `Not` recurse.
///
/// # Arguments
///
/// * `cond` - the condition under testing.
/// * `rt` - the runtime facts under reading.
/// * `fs` - the backend under stating.
///
/// # Returns
///
/// True while the condition holds.
///
/// # Examples
///
/// ```text
/// use confit_core::document::Condition;
/// use confit_core::fs::MemoryFs;
/// use confit_core::runtime::{Runtime, evaluate};
///
/// let rt = Runtime { vars: Default::default(), path_dirs: Vec::new() };
/// let cond = Condition::EnvSet { key: "HOME".into() };
/// assert!(matches!(evaluate(&cond, &rt, &MemoryFs::new()), false));
/// ```
pub fn evaluate(cond: &Condition, rt: &Runtime, fs: &dyn Filesystem) -> bool {
    match cond {
        Condition::EnvEq { key, value } => rt.vars.get(key).is_some_and(|held| held == value),
        Condition::EnvSet { key } => rt.vars.get(key).is_some_and(|held| !held.is_empty()),
        Condition::InPath { name } => path_holds(name, rt, fs),
        Condition::Exists { path } => fs.exists(&DocPath::new(path).expand()),
        Condition::All(items) => items.iter().all(|item| evaluate(item, rt, fs)),
        Condition::Any(items) => items.iter().any(|item| evaluate(item, rt, fs)),
        Condition::Not(inner) => !evaluate(inner, rt, fs),
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
/// # Examples
///
/// ```text
/// use confit_core::fs::MemoryFs;
/// use confit_core::runtime::find_binary;
/// use std::path::PathBuf;
///
/// let found = find_binary("tool", &[PathBuf::from("/bin")], &MemoryFs::new());
/// assert!(matches!(found, None));
/// ```
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

/// Parses one Lua-shaped duration into seconds.
///
/// Units read `h`, `m`, `s` in that order, repeats allowed
/// like `1h10m10s`. Bare digits read as seconds. Each unit
/// holds at most once.
///
/// # Arguments
///
/// * `text` - the raw duration text.
///
/// # Returns
///
/// The duration in seconds.
///
/// # Errors
///
/// Empty plus garbage plus wrong order fail with the text quoted.
///
/// # Examples
///
/// ```text
/// use confit_core::runtime::parse_duration;
///
/// assert!(matches!(parse_duration("10m"), Ok(600)));
/// assert!(matches!(parse_duration("90"), Ok(90)));
/// assert!(matches!(parse_duration("nope"), Err(_)));
/// ```
pub fn parse_duration(text: &str) -> Result<u64, String> {
    if text.is_empty() {
        return Err(format!("invalid duration '{text}'"));
    }
    let mut total: u64 = 0;
    let mut rank: u8 = 4;
    let mut seen: u8 = 0;
    let mut rest = text;
    let mut consumed_any = false;
    while !rest.is_empty() {
        let digits = rest.len()
            - rest
                .trim_start_matches(|byte: char| byte.is_ascii_digit())
                .len();
        if digits == 0 {
            return Err(format!("invalid duration '{text}'"));
        }
        let amount: u64 = match rest[..digits].parse() {
            Ok(amount) => amount,
            Err(_) => return Err(format!("invalid duration '{text}'")),
        };
        rest = &rest[digits..];
        let (unit_rank, unit_bit, factor) = match rest.chars().next() {
            Some('h') => (3, 0b100, 3_600),
            Some('m') => (2, 0b010, 60),
            Some('s') => (1, 0b001, 1),
            _ => (0, 0b000, 1),
        };
        if unit_rank == 0 {
            if consumed_any || !rest.is_empty() {
                return Err(format!("invalid duration '{text}'"));
            }
            total = amount;
            consumed_any = true;
            rest = "";
            continue;
        }
        if unit_rank >= rank || seen & unit_bit != 0 {
            return Err(format!("invalid duration '{text}'"));
        }
        rank = unit_rank;
        seen |= unit_bit;
        let part = match amount.checked_mul(factor) {
            Some(part) => part,
            None => return Err(format!("invalid duration '{text}'")),
        };
        total = match total.checked_add(part) {
            Some(total) => total,
            None => return Err(format!("invalid duration '{text}'")),
        };
        rest = &rest[1..];
        consumed_any = true;
    }
    if consumed_any {
        Ok(total)
    } else {
        Err(format!("invalid duration '{text}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::MemoryFs;

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
        ];
        for (cond, want) in cases {
            assert_eq!(evaluate(&cond, &rt, &fs), want, "condition {cond:?}");
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
        assert!(evaluate(
            &Condition::InPath { name: "dup".into() },
            &rt,
            &fs
        ));
    }

    #[test]
    fn in_path_without_mode_reports_presence() {
        let fs = MemoryFs::new();
        let rt = test_runtime(&fs);
        assert!(evaluate(
            &Condition::InPath {
                name: "tool".into()
            },
            &rt,
            &fs
        ));
        let bare = Runtime {
            vars: BTreeMap::new(),
            path_dirs: vec![PathBuf::from("/opt")],
        };
        assert!(evaluate(
            &Condition::InPath {
                name: "tool".into()
            },
            &bare,
            &fs
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
        let found = evaluate(
            &Condition::Exists {
                path: "~/.local/bin/hook".into(),
            },
            &rt,
            &fs,
        );
        let missing = evaluate(
            &Condition::Exists {
                path: "~/.local/bin/absent".into(),
            },
            &rt,
            &fs,
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

    #[test]
    fn duration_parser_cases() {
        let cases = vec![
            ("90", 90),
            ("0", 0),
            ("10s", 10),
            ("10m", 600),
            ("2h", 7_200),
            ("1h10m10s", 4_210),
            ("1h30m", 5_400),
        ];
        for (text, want) in cases {
            match parse_duration(text) {
                Ok(got) => assert_eq!(got, want, "duration {text:?}"),
                Err(error) => panic!("duration {text:?} parses: {error}"),
            }
        }
    }

    #[test]
    fn duration_parser_failures_quote_text() {
        for text in [
            "", "nope", "h", "10x", "10s1h", "1m1h", "1h1h", "1h30", " 10m", "10m ",
        ] {
            match parse_duration(text) {
                Ok(got) => panic!("duration {text:?} passes with {got}"),
                Err(error) => assert!(
                    error.contains(text) && error.contains('\''),
                    "failure quotes text: {error}"
                ),
            }
        }
    }
}
