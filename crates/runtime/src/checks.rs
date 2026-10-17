//! Host facts for condition verdicts and hook timeouts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use confit_driver as driver;
use confit_model::condition::Condition;
use confit_model::routes::Route;

/// Default hook timeout in seconds for the `10m` opt default.
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 600;

/// One unmet condition with its failing member.
///
/// - Conjunction faults hold the first failing member.
/// - Disjunction faults hold every member fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    /// The failing member index in its parent conjunction.
    pub index: usize,
    /// The failing member condition.
    pub member: Condition,
    /// The nested faults under the member.
    pub cause: Vec<Unmet>,
}

/// Host facts under condition checks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checks {
    /// Environment variables under reading.
    pub vars: BTreeMap<String, String>,
    /// PATH directories in search order.
    pub path_dirs: Vec<PathBuf>,
}

impl Checks {
    /// Snapshot the host environment and PATH dirs.
    ///
    /// - Non-Unicode entries drop.
    /// - Missing PATH is empty.
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

    /// Return facts with extra dirs first.
    pub fn extended(&self, dirs: Vec<PathBuf>) -> Self {
        let mut path_dirs = dirs;
        path_dirs.extend(self.path_dirs.iter().cloned());
        Self {
            vars: self.vars.clone(),
            path_dirs,
        }
    }

    /// Report whether conditions hold against facts and the driver.
    pub fn check(&self, cond: &Condition, changed: &BTreeSet<Route>) -> bool {
        match cond {
            Condition::EnvEq { key, value } => self.vars.get(key).is_some_and(|held| held == value),
            Condition::EnvSet { key } => self.vars.get(key).is_some_and(|held| !held.is_empty()),
            Condition::InPath { name } => find_executable(name, &self.path_dirs).is_some(),
            Condition::Exists { route } => route.expand().is_ok_and(|path| path.exists()),
            Condition::Changed { route } => changed.contains(route),
            Condition::All(items) => items.iter().all(|item| self.check(item, changed)),
            Condition::Any(items) => items.iter().any(|item| self.check(item, changed)),
            Condition::Not(inner) => !self.check(inner, changed),
        }
    }

    /// Return the fault under one condition while the condition fails.
    pub fn unmet(&self, cond: &Condition, changed: &BTreeSet<Route>) -> Option<Unmet> {
        match cond {
            Condition::All(items) => self.conjunction_unmet(items, changed),
            Condition::Any(items) => self.disjunction_unmet(cond, items, changed),
            Condition::Not(inner) => self.negation_unmet(cond, inner, changed),
            leaf => self.leaf_unmet(leaf, changed),
        }
    }

    /// Return the fault at the first failing member of one conjunction.
    fn conjunction_unmet(&self, items: &[Condition], changed: &BTreeSet<Route>) -> Option<Unmet> {
        for (index, member) in items.iter().enumerate() {
            if self.check(member, changed) {
                continue;
            }
            return Some(Unmet {
                index,
                member: member.clone(),
                cause: self.nested(member, changed),
            });
        }
        None
    }

    /// Return the nested faults under one failing member.
    fn nested(&self, member: &Condition, changed: &BTreeSet<Route>) -> Vec<Unmet> {
        match member {
            Condition::All(items) => self.conjunction_unmet(items, changed).into_iter().collect(),
            Condition::Any(items) => self.disjunction_each(items, changed),
            _ => Vec::new(),
        }
    }

    /// Return the fault under one disjunction with every member fault.
    fn disjunction_unmet(
        &self,
        cond: &Condition,
        items: &[Condition],
        changed: &BTreeSet<Route>,
    ) -> Option<Unmet> {
        if items.iter().any(|item| self.check(item, changed)) {
            return None;
        }
        Some(Unmet {
            index: 0,
            member: cond.clone(),
            cause: self.disjunction_each(items, changed),
        })
    }

    /// Collect one fault per member under one failed disjunction.
    fn disjunction_each(&self, items: &[Condition], changed: &BTreeSet<Route>) -> Vec<Unmet> {
        items
            .iter()
            .filter_map(|item| self.unmet(item, changed))
            .collect()
    }

    /// Return the fault under one negation while inner conditions hold.
    fn negation_unmet(
        &self,
        cond: &Condition,
        inner: &Condition,
        changed: &BTreeSet<Route>,
    ) -> Option<Unmet> {
        if self.check(inner, changed) {
            Some(Unmet {
                index: 0,
                member: cond.clone(),
                cause: Vec::new(),
            })
        } else {
            None
        }
    }

    /// Return the fault under one leaf while the leaf fails.
    fn leaf_unmet(&self, leaf: &Condition, changed: &BTreeSet<Route>) -> Option<Unmet> {
        if self.check(leaf, changed) {
            None
        } else {
            Some(Unmet {
                index: 0,
                member: leaf.clone(),
                cause: Vec::new(),
            })
        }
    }
}

/// Find one executable across dirs through the driver.
///
/// - Ordered dirs win.
/// - Symlinked entries count as present.
/// - Modeless backends count presence as a hit.
pub fn find_executable(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
    const EXEC_BIT: u32 = 0o111;

    for dir in dirs {
        let candidate = dir.join(name);
        if !candidate.exists() {
            continue;
        }
        match driver::fs::mode(&candidate) {
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
    use confit_model::routes::{Route, RouteBase};

    fn literal(path: &std::path::Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn checks_with(dirs: Vec<PathBuf>) -> Checks {
        Checks {
            vars: BTreeMap::new(),
            path_dirs: dirs,
        }
    }

    fn place(path: &std::path::Path, mode: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"run").unwrap();
        driver::fs::set_mode(path, Some(mode)).unwrap();
    }

    /// Driver oracle for the discovery decision.
    #[cfg(unix)]
    fn oracle(name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
        const EXEC_BIT: u32 = 0o111;
        for dir in dirs {
            let candidate = dir.join(name);
            if !candidate.exists() {
                continue;
            }
            match driver::fs::mode(&candidate) {
                Ok(mode) if mode & EXEC_BIT != 0 => return Some(candidate),
                Ok(_) => {}
                Err(_) => return Some(candidate),
            }
        }
        None
    }

    #[cfg(unix)]
    #[test]
    fn executable_bit_controls_discovery() {
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
            ),
            "InPath holds a 755 hit"
        );
        assert!(
            !checks_with(dirs).check(
                &Condition::InPath {
                    name: "regular".into()
                },
                &BTreeSet::new(),
            ),
            "InPath misses a 644 file"
        );
    }

    #[test]
    fn ordered_dirs_first_win() {
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

    #[cfg(unix)]
    #[test]
    fn symlinks_follow_with_dangling_missing() {
        let dir = tempfile::tempdir().unwrap();
        place(&dir.path().join("tool"), 0o755);
        driver::fs::write_link(&dir.path().join("link_ok"), std::path::Path::new("tool")).unwrap();
        driver::fs::set_mode(&dir.path().join("link_ok"), Some(0o755)).unwrap();
        driver::fs::write_link(
            &dir.path().join("link_dead"),
            &dir.path().join("dangling-target"),
        )
        .unwrap();
        let dirs = vec![dir.path().to_path_buf()];
        assert_eq!(
            find_executable("link_ok", &dirs),
            Some(dir.path().join("link_ok")),
            "current link follows its target"
        );
        assert_eq!(
            find_executable("link_dead", &dirs),
            None,
            "dangling link is missing"
        );
    }

    #[cfg(unix)]
    #[test]
    fn helper_matches_oracle_row_by_row() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        for folder in [&first, &second] {
            std::fs::create_dir_all(folder).unwrap();
        }
        place(&first.join("tool"), 0o755);
        place(&first.join("regular"), 0o644);
        place(&second.join("tool"), 0o755);
        driver::fs::write_link(&first.join("link_ok"), std::path::Path::new("tool")).unwrap();
        driver::fs::write_link(&first.join("link_plain"), std::path::Path::new("regular")).unwrap();
        driver::fs::write_link(&first.join("link_dead"), &first.join("dangling-target")).unwrap();
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
    fn exists_true_for_seeded_false_for_absent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("needle");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(&path, b"needle").unwrap();
        let route = literal(&path);
        assert_eq!(
            route.expand().unwrap(),
            path,
            "literal routes resolve unchanged"
        );
        assert!(
            checks_with(Vec::new()).check(
                &Condition::Exists {
                    route: route.clone()
                },
                &BTreeSet::new(),
            ),
            "seeded path presence holds"
        );
        let missing = literal(&dir.path().join("absent"));
        assert!(
            !checks_with(Vec::new())
                .check(&Condition::Exists { route: missing }, &BTreeSet::new(),),
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
            ),
            "member route holds"
        );
        assert!(
            !checks_with(Vec::new()).check(&Condition::Changed { route: other }, &changed,),
            "absent route fails"
        );
    }

    fn leaf(name: &str) -> Condition {
        Condition::InPath {
            name: name.to_owned(),
        }
    }

    #[test]
    fn unmet_conjunction_names_first_failing_member() {
        let checks = checks_with(Vec::new());
        let cond = Condition::All(vec![leaf("a"), leaf("b")]);
        let fault = checks.unmet(&cond, &BTreeSet::new()).unwrap();
        assert_eq!(fault.index, 0);
        assert_eq!(fault.member, leaf("a"));
        assert!(fault.cause.is_empty());
    }

    #[test]
    fn unmet_conjunction_carries_nested_cause() {
        let checks = checks_with(Vec::new());
        let held = Condition::Not(Box::new(leaf("absent")));
        let cond = Condition::All(vec![held, Condition::All(vec![leaf("b"), leaf("c")])]);
        let fault = checks.unmet(&cond, &BTreeSet::new()).unwrap();
        assert_eq!(fault.index, 1);
        assert_eq!(fault.member, Condition::All(vec![leaf("b"), leaf("c")]));
        assert_eq!(fault.cause.len(), 1);
        assert_eq!(fault.cause[0].index, 0);
        assert_eq!(fault.cause[0].member, leaf("b"));
    }

    #[test]
    fn unmet_disjunction_names_every_member() {
        let checks = checks_with(Vec::new());
        let cond = Condition::Any(vec![leaf("a"), leaf("b")]);
        let fault = checks.unmet(&cond, &BTreeSet::new()).unwrap();
        assert_eq!(fault.member, cond);
        assert_eq!(fault.cause.len(), 2);
        assert_eq!(fault.cause[0].member, leaf("a"));
        assert_eq!(fault.cause[1].member, leaf("b"));
    }

    #[test]
    fn unmet_holding_conditions_read_none() {
        let checks = checks_with(Vec::new());
        assert_eq!(
            checks.unmet(&Condition::All(Vec::new()), &BTreeSet::new()),
            None,
            "empty conjunction holds"
        );
        assert_eq!(
            checks.unmet(&Condition::Not(Box::new(leaf("absent"))), &BTreeSet::new()),
            None,
            "negated miss holds"
        );
    }
}
