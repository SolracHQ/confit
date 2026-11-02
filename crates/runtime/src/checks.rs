//! Host facts for condition verdicts and hook timeouts.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use confit_driver as driver;
use confit_model::condition::Condition;
use confit_model::routes::Route;

/// The hook timeout in seconds when a hook takes the `10m` default.
pub const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 600;

/// One unmet condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmet {
    /// The failing member index in its parent conjunction.
    pub index: usize,
    /// The failing member condition.
    pub member: Condition,
    /// The nested faults under the member.
    pub cause: Vec<Unmet>,
}

/// One snapshot of the host facts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checks {
    /// The host environment variables.
    pub vars: BTreeMap<String, String>,
    /// The directories that the PATH search visits in order.
    pub path_dirs: Vec<PathBuf>,
}

impl Checks {
    /// Snapshots the current host environment and the PATH directories.
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

    /// Returns the facts with the extra directories searched first.
    pub fn extended(&self, dirs: Vec<PathBuf>) -> Self {
        let mut path_dirs = dirs;
        path_dirs.extend(self.path_dirs.iter().cloned());
        Self {
            vars: self.vars.clone(),
            path_dirs,
        }
    }

    /// Returns whether one condition holds against the facts and the driver.
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

    /// Returns the fault of one condition that fails.
    pub fn unmet(&self, cond: &Condition, changed: &BTreeSet<Route>) -> Option<Unmet> {
        match cond {
            Condition::All(items) => self.conjunction_unmet(items, changed),
            Condition::Any(items) => self.disjunction_unmet(cond, items, changed),
            Condition::Not(inner) => self.negation_unmet(cond, inner, changed),
            leaf => self.leaf_unmet(leaf, changed),
        }
    }

    /// Returns the fault of the first failing member of one conjunction.
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

    /// Returns the faults nested under one failing member.
    fn nested(&self, member: &Condition, changed: &BTreeSet<Route>) -> Vec<Unmet> {
        match member {
            Condition::All(items) => self.conjunction_unmet(items, changed).into_iter().collect(),
            Condition::Any(items) => self.disjunction_each(items, changed),
            _ => Vec::new(),
        }
    }

    /// Returns the fault of one disjunction with the fault of every member.
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

    /// Collects one fault per member of one failed disjunction.
    fn disjunction_each(&self, items: &[Condition], changed: &BTreeSet<Route>) -> Vec<Unmet> {
        items
            .iter()
            .filter_map(|item| self.unmet(item, changed))
            .collect()
    }

    /// Returns the fault of one negation while its inner conditions hold.
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

    /// Returns the fault of one leaf that fails.
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

/// Collects the member strings of the failing leaves under one unmet fault.
pub fn unmet_names(fault: &Unmet) -> Vec<String> {
    if fault.cause.is_empty() {
        return vec![fault.member.describe()];
    }
    let mut out = Vec::new();
    for nested in &fault.cause {
        out.extend(unmet_names(nested));
    }
    out
}

/// Finds one executable in the directories through the driver.
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
    use std::path::Path;

    use confit_driver as driver;
    use confit_model::routes::{Route, RouteBase};

    fn literal(path: &Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn checks_with(dirs: Vec<PathBuf>) -> Checks {
        Checks {
            vars: BTreeMap::new(),
            path_dirs: dirs,
        }
    }

    fn place(path: &Path, mode: u32) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"run").unwrap();
        driver::fs::set_mode(path, Some(mode)).unwrap();
    }

    /// Returns the expected executable for the discovery decision.
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

    #[test]
    fn discovery_requires_the_executable_bit() {
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
            "the search skips a 644 file"
        );
        assert_eq!(
            find_executable("absent", &dirs),
            None,
            "the search skips an absent binary"
        );
        assert!(
            checks_with(dirs.clone()).check(
                &Condition::InPath {
                    name: "tool".into()
                },
                &BTreeSet::new(),
            ),
            "the path condition holds an executable file"
        );
        assert!(
            !checks_with(dirs).check(
                &Condition::InPath {
                    name: "regular".into()
                },
                &BTreeSet::new(),
            ),
            "the path condition rejects a 644 file"
        );
    }

    #[test]
    fn search_order_picks_the_first_directory() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        place(&first.join("tool"), 0o755);
        place(&second.join("tool"), 0o755);
        assert_eq!(
            find_executable("tool", &[first.clone(), second.clone()]),
            Some(first.join("tool")),
            "the first directory wins"
        );
        assert_eq!(
            find_executable("tool", &[second.clone(), first.clone()]),
            Some(second.join("tool")),
            "the reordered directories move the win"
        );
    }

    #[test]
    fn discovery_follows_a_working_link_and_skips_a_broken_link() {
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
            "the search follows a working link"
        );
        assert_eq!(
            find_executable("link_dead", &dirs),
            None,
            "the search skips a dangling link"
        );
    }

    #[test]
    fn find_executable_matches_the_oracle_on_every_row() {
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
            assert_eq!(oracle, found, "the helper matches the oracle for {name}");
            assert_eq!(found, want, "the search returns {name} as expected");
        }
    }

    #[test]
    fn exists_condition_holds_for_a_seeded_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("needle");
        std::fs::create_dir_all(dir.path()).unwrap();
        std::fs::write(&path, b"needle").unwrap();
        let route = literal(&path);
        assert_eq!(
            route.expand().unwrap(),
            path,
            "the literal route keeps its path"
        );
        assert!(
            checks_with(Vec::new()).check(
                &Condition::Exists {
                    route: route.clone()
                },
                &BTreeSet::new(),
            ),
            "the exists condition holds a seeded path"
        );
        let missing = literal(&dir.path().join("absent"));
        assert!(
            !checks_with(Vec::new())
                .check(&Condition::Exists { route: missing }, &BTreeSet::new(),),
            "the exists condition rejects an absent path"
        );
    }

    #[test]
    fn changed_condition_follows_route_membership() {
        let route = literal(Path::new("/opt/probe/touched"));
        let other = literal(Path::new("/opt/probe/untouched"));
        let changed = BTreeSet::from([route.clone()]);
        assert!(
            checks_with(Vec::new()).check(
                &Condition::Changed {
                    route: route.clone()
                },
                &changed,
            ),
            "the changed condition holds a member route"
        );
        assert!(
            !checks_with(Vec::new()).check(&Condition::Changed { route: other }, &changed,),
            "the changed condition rejects an absent route"
        );
    }

    fn leaf(name: &str) -> Condition {
        Condition::InPath {
            name: name.to_owned(),
        }
    }

    #[test]
    fn unmet_conjunction_names_the_first_unmet_member() {
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
    fn unmet_holding_conditions_return_none() {
        let checks = checks_with(Vec::new());
        assert_eq!(
            checks.unmet(&Condition::All(Vec::new()), &BTreeSet::new()),
            None,
            "the empty conjunction holds"
        );
        assert_eq!(
            checks.unmet(&Condition::Not(Box::new(leaf("absent"))), &BTreeSet::new()),
            None,
            "the negated condition holds"
        );
    }
}
