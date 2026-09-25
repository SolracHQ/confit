//! Checks
//!
//! Host facts behind condition verdicts and hook timeouts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use confit_driver as driver;
use confit_model::condition::Condition;
use confit_model::handles::Route;

use crate::Applier;

/// Default hook timeout in seconds backing the `10m` opt default.
///
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 600;

/// Host facts under condition checks.
///
/// Vars hold the process environment snapshot. Path dirs hold
/// the PATH entries in order. Tests build fixed values, so
/// checks never read ambient state.
///
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checks {
    /// Holds the environment variables under reading.
    pub vars: BTreeMap<String, String>,
    /// Holds the PATH directories in search order.
    pub path_dirs: Vec<PathBuf>,
}

impl Checks {
    /// Snapshots the host environment and PATH dirs.
    ///
    /// Non-Unicode entries drop. Missing PATH reads as empty.
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

    /// Reports one condition verdict against facts and the driver.
    ///
    /// `Exists` reads the applier-expanded path, so checks never
    /// name an unexpanded route. `Changed` reads membership in the
    /// changed route set. `All`, `Any` and `Not` recurse.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::condition::Condition;
    /// use confit_runtime::checks::Checks;
    /// use std::collections::{BTreeMap, BTreeSet};
    ///
    /// let checks = Checks {
    ///     vars: BTreeMap::from([("SHELL".to_string(), "bash".to_string())]),
    ///     path_dirs: Vec::new(),
    /// };
    /// let cond = Condition::All(vec![
    ///     Condition::EnvSet { key: "SHELL".into() },
    ///     Condition::EnvEq { key: "SHELL".into(), value: "bash".into() },
    /// ]);
    /// let applier = confit_runtime::Applier::host(confit_store::StoreRoots::default());
    /// assert!(checks.check(&cond, &BTreeSet::new(), &applier));
    /// assert!(!checks.check(
    ///     &Condition::EnvSet { key: "MISSING".into() },
    ///     &BTreeSet::new(),
    ///     &applier
    /// ));
    /// ```
    pub fn check(&self, cond: &Condition, changed: &BTreeSet<Route>, applier: &Applier) -> bool {
        match cond {
            Condition::EnvEq { key, value } => self.vars.get(key).is_some_and(|held| held == value),
            Condition::EnvSet { key } => self.vars.get(key).is_some_and(|held| !held.is_empty()),
            Condition::InPath { name } => find_executable(name, &self.path_dirs).is_some(),
            Condition::Exists { route } => driver::exists(&applier.resolve(route)),
            Condition::Changed { route } => changed.contains(route),
            Condition::All(items) => items.iter().all(|item| self.check(item, changed, applier)),
            Condition::Any(items) => items.iter().any(|item| self.check(item, changed, applier)),
            Condition::Not(inner) => !self.check(inner, changed, applier),
        }
    }
}

/// Finds one executable across dirs through the driver.
///
/// Ordered dirs win, so the first hit returns. Existence
/// follows symlinks, so dangling links miss. Modeless
/// backends count presence as a hit.
pub fn find_executable(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    const EXEC_BIT: u32 = 0o111;

    for dir in dirs {
        let candidate = dir.join(name);
        if !driver::exists(&candidate) {
            continue;
        }
        match driver::mode(&candidate) {
            Ok(mode) if mode & EXEC_BIT != 0 => return Some(candidate),
            Ok(_) => {}
            Err(_) => return Some(candidate),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver as driver;
    use confit_driver::TestGuard;
    use confit_model::handles::{Route, RouteBase};

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn checks_with(dirs: Vec<PathBuf>) -> Checks {
        Checks {
            vars: BTreeMap::new(),
            path_dirs: dirs,
        }
    }

    fn applier() -> Applier {
        Applier::host(confit_store::StoreRoots::default())
    }

    fn place(path: &std::path::Path, mode: u32) {
        driver::create_dir_all(path.parent().unwrap()).unwrap();
        driver::write(path, b"run").unwrap();
        driver::set_mode(path, mode).unwrap();
    }

    /// Driver oracle replaying the discovery decision branch by branch.
    ///
    /// Present entries hit while an exec bit holds.
    /// Modeless backends count presence.
    fn oracle(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
        const EXEC_BIT: u32 = 0o111;
        for dir in dirs {
            let candidate = dir.join(name);
            if !driver::exists(&candidate) {
                continue;
            }
            match driver::mode(&candidate) {
                Ok(mode) if mode & EXEC_BIT != 0 => return Some(candidate),
                Ok(_) => {}
                Err(_) => return Some(candidate),
            }
        }
        None
    }

    #[test]
    fn executable_bit_gates_discovery() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("tool");
        let regular = dir.path().join("regular");
        place(&tool, 0o755);
        place(&regular, 0o644);
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(find_executable("tool", &dirs), Some(tool));
        assert_eq!(
            find_executable("regular", &dirs),
            None,
            "644 without exec bit misses"
        );
        assert_eq!(
            find_executable("absent", &dirs),
            None,
            "missing binary misses"
        );
        assert!(
            checks_with(dirs.clone()).check(
                &Condition::InPath {
                    name: "tool".into()
                },
                &BTreeSet::new(),
                &applier()
            ),
            "InPath holds a 755 hit"
        );
        assert!(
            !checks_with(dirs).check(
                &Condition::InPath {
                    name: "regular".into()
                },
                &BTreeSet::new(),
                &applier()
            ),
            "InPath misses a 644 file"
        );
    }

    #[test]
    fn ordered_dirs_first_win() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        place(&first.join("tool"), 0o755);
        place(&second.join("tool"), 0o755);
        assert_eq!(
            find_executable("tool", &[first.clone(), second.clone()]),
            Some(first.join("tool")),
            "first dir wins"
        );
        assert_eq!(
            find_executable("tool", &[second.clone(), first.clone()]),
            Some(second.join("tool")),
            "order flip moves the win"
        );
    }

    #[test]
    fn symlinks_follow_with_dangling_missing() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        place(&dir.path().join("tool"), 0o755);
        driver::write_link(&dir.path().join("link_ok"), std::path::Path::new("tool")).unwrap();
        driver::set_mode(&dir.path().join("link_ok"), 0o755).unwrap();
        driver::write_link(
            &dir.path().join("link_dead"),
            &dir.path().join("dangling-target"),
        )
        .unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(
            find_executable("link_ok", &dirs),
            Some(dir.path().join("link_ok")),
            "live link follows its target"
        );
        assert_eq!(
            find_executable("link_dead", &dirs),
            None,
            "dangling link reads as missing"
        );
    }

    #[test]
    fn helper_matches_oracle_row_by_row() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        for folder in [&first, &second] {
            driver::create_dir_all(folder).unwrap();
        }
        place(&first.join("tool"), 0o755);
        place(&first.join("regular"), 0o644);
        place(&second.join("tool"), 0o755);
        driver::write_link(&first.join("link_ok"), std::path::Path::new("tool")).unwrap();
        driver::set_mode(&first.join("link_ok"), 0o755).unwrap();
        driver::write_link(&first.join("link_plain"), std::path::Path::new("regular")).unwrap();
        driver::set_mode(&first.join("link_plain"), 0o755).unwrap();
        driver::write_link(&first.join("link_dead"), &first.join("dangling-target")).unwrap();
        let rows: Vec<(&str, Vec<PathBuf>, Option<PathBuf>)> = vec![
            ("tool", vec![first.clone()], Some(first.join("tool"))),
            ("regular", vec![first.clone()], None),
            ("absent", vec![first.clone()], None),
            ("link_ok", vec![first.clone()], Some(first.join("link_ok"))),
            (
                "link_plain",
                vec![first.clone()],
                Some(first.join("link_plain")),
            ),
            ("link_dead", vec![first.clone()], None),
            (
                "tool",
                vec![first.clone(), second.clone()],
                Some(first.join("tool")),
            ),
            (
                "tool",
                vec![second.clone(), first.clone()],
                Some(second.join("tool")),
            ),
        ];
        for (name, dirs, want) in rows {
            let oracle = oracle(name, &dirs);
            let found = find_executable(name, &dirs);
            assert_eq!(oracle, found, "helper parity for {name}");
            assert_eq!(found, want, "helper verdict for {name}");
        }
    }

    #[test]
    fn exists_reads_seeded_plus_absent() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("needle");
        driver::create_dir_all(dir.path()).unwrap();
        driver::write(&path, b"needle").unwrap();
        let route = literal(&path);
        let expanded = applier().resolve(&route);
        assert_eq!(expanded, path, "literal routes resolve verbatim");
        assert!(
            checks_with(Vec::new()).check(
                &Condition::Exists {
                    route: route.clone()
                },
                &BTreeSet::new(),
                &applier()
            ),
            "seeded path presence holds"
        );
        let missing = literal(&dir.path().join("absent"));
        assert!(
            !checks_with(Vec::new()).check(
                &Condition::Exists { route: missing },
                &BTreeSet::new(),
                &applier()
            ),
            "absent path fails"
        );
    }

    #[test]
    fn changed_reads_route_membership() {
        let route = literal(std::path::Path::new("/opt/probe/touched"));
        let other = literal(std::path::Path::new("/opt/probe/untouched"));
        let changed = BTreeSet::from([route.clone()]);
        assert!(
            checks_with(Vec::new()).check(
                &Condition::Changed {
                    route: route.clone()
                },
                &changed,
                &applier()
            ),
            "member route holds"
        );
        assert!(
            !checks_with(Vec::new()).check(
                &Condition::Changed { route: other },
                &changed,
                &applier()
            ),
            "absent route fails"
        );
    }
}
