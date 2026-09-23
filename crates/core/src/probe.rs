//! Probe
//!
//! Read-only path facts behind condition evaluation.

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Read-only path facts behind condition evaluation.
pub trait PathProbe {
    /// Reports path presence.
    fn exists(&self, path: &Path) -> bool;

    /// Finds one executable across dirs.
    fn find_executable(&self, name: &str, dirs: &[PathBuf]) -> Option<PathBuf>;
}

/// Host path probe reading the process filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsProbe;

impl PathProbe for OsProbe {
    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn find_executable(&self, name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
        const EXEC_BIT: u32 = 0o111;

        for dir in dirs {
            let candidate = dir.join(name);
            match std::fs::symlink_metadata(&candidate) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
                Ok(meta) => {
                    if meta.permissions().mode() & EXEC_BIT != 0 {
                        return Some(candidate);
                    }
                }
                Err(_) => {
                    if candidate.exists() {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }
}

/// Memory path facts for tests.
#[derive(Debug, Clone, Default)]
pub struct MemoryProbe {
    entries: BTreeMap<PathBuf, Entry>,
}

/// One memory probe entry.
#[derive(Debug, Clone)]
enum Entry {
    /// File presence plus mode.
    File { mode: Option<u32> },
    /// Link target.
    Link { target: PathBuf },
}

impl MemoryProbe {
    /// Builds an empty memory probe.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one file.
    pub fn file(&mut self, path: &Path) -> &mut Self {
        self.entries
            .insert(path.to_path_buf(), Entry::File { mode: None });
        self
    }

    /// Records one executable file.
    pub fn exec(&mut self, path: &Path) -> &mut Self {
        self.entries
            .insert(path.to_path_buf(), Entry::File { mode: Some(0o755) });
        self
    }

    /// Records one symlink.
    pub fn link(&mut self, link: &Path, target: &Path) -> &mut Self {
        self.entries.insert(
            link.to_path_buf(),
            Entry::Link {
                target: target.to_path_buf(),
            },
        );
        self
    }

    /// Records unix permission bits on one path.
    pub fn mode(&mut self, path: &Path, mode: u32) -> &mut Self {
        match self.entries.get_mut(path) {
            Some(Entry::File { mode: held }) => *held = Some(mode),
            _ => {
                self.entries
                    .insert(path.to_path_buf(), Entry::File { mode: Some(mode) });
            }
        }
        self
    }
}

impl PathProbe for MemoryProbe {
    /// Reports memory path presence.
    fn exists(&self, path: &Path) -> bool {
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut next = path.to_path_buf();
        loop {
            match self.entries.get(&next) {
                Some(Entry::File { .. }) => return true,
                Some(Entry::Link { target }) => {
                    let target = target.clone();
                    if !seen.insert(next.clone()) {
                        return false;
                    }
                    next = if target.is_absolute() {
                        target
                    } else {
                        match next.parent() {
                            Some(parent) => parent.join(target),
                            None => target,
                        }
                    };
                }
                None => return false,
            }
        }
    }

    /// Finds one executable across dirs.
    fn find_executable(&self, name: &str, dirs: &[PathBuf]) -> Option<PathBuf> {
        const EXEC_BIT: u32 = 0o111;

        for dir in dirs {
            let candidate = dir.join(name);
            match self.entries.get(&candidate) {
                Some(Entry::File { mode: Some(mode) }) => {
                    if mode & EXEC_BIT != 0 {
                        return Some(candidate);
                    }
                }
                _ => {
                    if self.exists(&candidate) {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hook::{Hook, resolve_hook};
    use crate::runtime::Runtime;

    struct Fixture {
        probe: MemoryProbe,
        hook: PathBuf,
        first: PathBuf,
        second: PathBuf,
    }

    fn fixture() -> (tempfile::TempDir, Fixture) {
        let guard = match tempfile::tempdir() {
            Ok(dir) => dir,
            Err(error) => panic!("scratch dir builds: {error}"),
        };
        let hook = guard.path().join("hook");
        let first = guard.path().join("first");
        let second = guard.path().join("second");
        for dir in [&hook, &first, &second] {
            match std::fs::create_dir_all(dir) {
                Ok(()) => {}
                Err(error) => panic!("scratch dir builds {}: {error}", dir.display()),
            }
        }
        let mut probe = MemoryProbe::new();
        place(&mut probe, &hook.join("tool"), 0o755);
        place(&mut probe, &first.join("tool"), 0o755);
        place(&mut probe, &first.join("regular"), 0o644);
        place(&mut probe, &second.join("tool"), 0o755);
        link(&mut probe, &first.join("link_ok"), Path::new("tool"));
        link(&mut probe, &first.join("link_plain"), Path::new("regular"));
        link(
            &mut probe,
            &first.join("link_dead"),
            Path::new("dangling-target"),
        );
        (
            guard,
            Fixture {
                probe,
                hook,
                first,
                second,
            },
        )
    }

    fn place(probe: &mut MemoryProbe, path: &Path, mode: u32) {
        probe.file(path).mode(path, mode);
        match std::fs::write(path, b"run") {
            Ok(_) => {}
            Err(error) => panic!("disk writes {}: {error}", path.display()),
        }
        use std::os::unix::fs::PermissionsExt as _;

        match std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)) {
            Ok(()) => {}
            Err(error) => panic!("disk modes {}: {error}", path.display()),
        }
    }

    fn link(probe: &mut MemoryProbe, link: &Path, target: &Path) {
        probe.link(link, target);
        match std::os::unix::fs::symlink(target, link) {
            Ok(()) => {}
            Err(error) => panic!("disk links {}: {error}", link.display()),
        }
    }

    fn hook_with(path: Vec<String>) -> Hook {
        Hook {
            argv: vec!["tool".to_string()],
            path,
            requires: None,
            when: None,
            checks: Vec::new(),
            timeout_secs: 60,
        }
    }

    #[test]
    fn differential_exists_matches_disk() {
        let (_dir, fx) = fixture();
        let os = OsProbe;
        let rows: Vec<(PathBuf, bool)> = vec![
            (fx.first.join("regular"), true),
            (fx.first.join("tool"), true),
            (fx.first.join("link_ok"), true),
            (fx.first.join("link_plain"), true),
            (fx.first.join("link_dead"), false),
            (fx.first.join("absent"), false),
        ];
        for (path, want) in rows {
            let disk = PathProbe::exists(&os, &path);
            let mem = PathProbe::exists(&fx.probe, &path);
            assert_eq!(disk, mem, "exists parity {}", path.display());
            assert_eq!(mem, want, "exists {}", path.display());
        }
    }

    #[test]
    fn differential_find_matches_disk() {
        let (_dir, fx) = fixture();
        let os = OsProbe;
        let rows: Vec<(&str, Vec<PathBuf>, Option<PathBuf>)> = vec![
            ("tool", vec![fx.first.clone()], Some(fx.first.join("tool"))),
            ("regular", vec![fx.first.clone()], None),
            ("absent", vec![fx.first.clone()], None),
            (
                "link_ok",
                vec![fx.first.clone()],
                Some(fx.first.join("link_ok")),
            ),
            (
                "link_plain",
                vec![fx.first.clone()],
                Some(fx.first.join("link_plain")),
            ),
            ("link_dead", vec![fx.first.clone()], None),
            (
                "tool",
                vec![fx.first.clone(), fx.second.clone()],
                Some(fx.first.join("tool")),
            ),
            (
                "tool",
                vec![fx.second.clone(), fx.first.clone()],
                Some(fx.second.join("tool")),
            ),
        ];
        for (name, dirs, want) in rows {
            let disk = PathProbe::find_executable(&os, name, &dirs);
            let mem = PathProbe::find_executable(&fx.probe, name, &dirs);
            assert_eq!(disk, mem, "find parity {name}");
            assert_eq!(mem, want, "find {name}");
        }
    }

    #[test]
    fn differential_hook_dirs_win_over_path_dirs() {
        let (_dir, fx) = fixture();
        let os = OsProbe;
        let rt = Runtime {
            vars: Default::default(),
            path_dirs: vec![fx.first.clone(), fx.second.clone()],
        };
        let hooked = hook_with(vec![fx.hook.display().to_string()]);
        let disk = resolve_hook(&hooked, &rt, &os);
        let mem = resolve_hook(&hooked, &rt, &fx.probe);
        assert_eq!(disk, mem, "hooked parity");
        assert_eq!(mem, Some(fx.hook.join("tool")), "hook dir wins");
        let bare = hook_with(Vec::new());
        let disk = resolve_hook(&bare, &rt, &os);
        let mem = resolve_hook(&bare, &rt, &fx.probe);
        assert_eq!(disk, mem, "path parity");
        assert_eq!(mem, Some(fx.first.join("tool")), "first path dir wins");
    }
}
