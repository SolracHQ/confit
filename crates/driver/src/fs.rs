//! Filesystem verbs over host files with a memory backend under tests.

use std::io;
use std::path::Path;

#[cfg(not(any(test, feature = "test-support")))]
mod host;
#[cfg(not(any(test, feature = "test-support")))]
pub use host::*;

#[cfg(any(test, feature = "test-support"))]
mod mem;
#[cfg(any(test, feature = "test-support"))]
pub use mem::*;

/// Filesystem facts for one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    len: u64,
    is_dir: bool,
    modified: u64,
}

impl FileMeta {
    /// Build facts from a byte length, the folder flag, and mtime nanos.
    pub fn new(len: u64, is_dir: bool, modified: u64) -> Self {
        Self {
            len,
            is_dir,
            modified,
        }
    }

    /// Return the byte length of one file, zero for folders.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Report whether the length is empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Report whether the path is a folder.
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    /// Report whether the path is a plain file.
    pub fn is_file(&self) -> bool {
        !self.is_dir
    }

    /// Return mtime nanos since the epoch, zero when unknown.
    pub fn modified(&self) -> u64 {
        self.modified
    }
}

/// Seekable file handle shared by host and memory backends.
///
/// Handles serve reads and writes and seeks over one cursor. Length report
/// committed bytes, sync land buffered bytes onto the backend.
#[allow(clippy::len_without_is_empty)]
pub trait FsFile: io::Read + io::Write + io::Seek {
    /// Return the committed byte length of one file.
    ///
    /// # Errors
    ///
    /// - [Other] when host metadata faults.
    fn len(&self) -> io::Result<u64>;

    /// Land buffered bytes onto the backend.
    ///
    /// # Errors
    ///
    /// - [InvalidInput] when the path refuses on memory.
    /// - [Other] when the home misses on memory and
    ///   when host faults strike.
    fn sync(&mut self) -> io::Result<()>;
}

impl FsFile for Box<dyn FsFile> {
    fn len(&self) -> io::Result<u64> {
        (**self).len()
    }

    fn sync(&mut self) -> io::Result<()> {
        (**self).sync()
    }
}

/// Report whether one path holds a folder.
pub fn is_dir(path: &Path) -> bool {
    metadata(path).is_ok_and(|facts| facts.is_dir())
}

/// Report whether one path holds a plain file.
pub fn is_file(path: &Path) -> bool {
    metadata(path).is_ok_and(|facts| facts.is_file())
}

/// Report whether one path holds a symlink.
pub fn is_link(path: &Path) -> bool {
    read_link(path).is_ok()
}

/// Report whether one path is a missing path.
pub fn is_missing(path: &Path) -> bool {
    !exists(path)
}

/// Build the parent folder for one file path.
///
/// - Empty and parentless paths skip.
///
/// # Errors
///
/// - Parent build failures fail as io errors.
pub fn create_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        create_dir_all(parent)?;
    }
    Ok(())
}

/// Remove one symlink without touching other entries.
///
/// - Links remove, other entries stay.
///
/// # Errors
///
/// - Link check and removal failures fail as io errors.
pub fn remove_link(path: &Path) -> io::Result<()> {
    if read_link(path).is_ok() {
        remove_file(path)?;
    }
    Ok(())
}

/// Read one symlink target as bytes.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [PermissionDenied] when the path refuses reads.
/// - [InvalidInput] for non-link paths.
pub fn read_link_bytes(path: &Path) -> io::Result<Vec<u8>> {
    Ok(read_link(path)?.as_os_str().as_encoded_bytes().to_vec())
}

/// Set unix permission bits for one optional mode.
///
/// - Absent modes skip.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [PermissionDenied] when the path refuses writes.
pub fn set_mode_if_present(path: &Path, mode: Option<u32>) -> io::Result<()> {
    if let Some(bits) = mode {
        set_mode(path, bits)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn guarded_writes_land_in_memory_never_host() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache").join("probe.bin");
        let parent = file.parent().unwrap().to_path_buf();
        create_dir_all(&parent).unwrap();
        write(&file, b"guarded bytes").unwrap();
        assert_eq!(read(&file).unwrap(), b"guarded bytes".to_vec());
        assert!(exists(&file));
        let facts = metadata(&file).unwrap();
        assert_eq!(facts.len(), 13);
        assert!(facts.is_file());
        let mut opened = open(&file).unwrap();
        let mut found = Vec::new();
        std::io::Read::read_to_end(&mut opened, &mut found).unwrap();
        assert_eq!(found, b"guarded bytes".to_vec());
        let stream = parent.join("stream.bin");
        {
            let mut out = create(&stream).unwrap();
            std::io::Write::write_all(&mut out, b"streamed").unwrap();
            std::io::Write::flush(&mut out).unwrap();
        }
        {
            let mut out = append(&stream).unwrap();
            std::io::Write::write_all(&mut out, b"and more").unwrap();
            std::io::Write::flush(&mut out).unwrap();
        }
        let moved = parent.join("moved.bin");
        rename(&stream, &moved).unwrap();
        assert!(!exists(&stream));
        assert_eq!(read(&moved).unwrap(), b"streamedand more".to_vec());
        remove_file(&moved).unwrap();
        assert!(!exists(&moved));
        assert_eq!(read_dir(&parent).unwrap(), vec![file.clone()]);
        for path in [&file, &stream, &moved] {
            assert!(
                !path.exists(),
                "guarded write never lands on host: {}",
                path.display()
            );
        }
        assert!(
            !dir.path().join("cache").exists(),
            "guarded folders never land on host"
        );
    }

    #[test]
    fn writes_bump_modified_ticks() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ticks").join("probe.bin");
        create_dir_all(file.parent().unwrap()).unwrap();
        write(&file, b"first").unwrap();
        let first = metadata(&file).unwrap().modified();
        assert_ne!(first, 0, "writes stamp ticks");
        write(&file, b"second").unwrap();
        let second = metadata(&file).unwrap().modified();
        assert!(second > first, "rewrites bump ticks");
    }

    #[test]
    fn fs_file_round_trip_seeks_lens_and_syncs() {
        use std::io::{Read as _, Seek as _, SeekFrom, Write as _};

        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("round").join("trip.bin");
        create_dir_all(file.parent().unwrap()).unwrap();
        let mut handle = create(&file).unwrap();
        handle.write_all(b"seekable bytes").unwrap();
        handle.sync().unwrap();
        assert_eq!(handle.len().unwrap(), 14);
        handle.seek(SeekFrom::Start(0)).unwrap();
        let mut found = Vec::new();
        handle.read_to_end(&mut found).unwrap();
        assert_eq!(found, b"seekable bytes".to_vec());
        drop(handle);
        let mut opened = open(&file).unwrap();
        assert_eq!(opened.len().unwrap(), 14);
        let mut found = Vec::new();
        opened.read_to_end(&mut found).unwrap();
        assert_eq!(found, b"seekable bytes".to_vec());
        drop(opened);
        let mut appended = append(&file).unwrap();
        appended.write_all(b"and more").unwrap();
        appended.sync().unwrap();
        assert_eq!(appended.len().unwrap(), 22);
        drop(appended);
        assert_eq!(read(&file).unwrap(), b"seekable bytesand more".to_vec());
        assert!(
            !file.exists(),
            "guarded handle never lands on host: {}",
            file.display()
        );
    }

    #[test]
    fn without_guard_host_write_fails_loud() {
        let probe = PathBuf::from("/proc/confit-guard-probe-no-guard/file.bin");
        let outcome = write(&probe, b"bytes");
        assert!(
            outcome.is_err(),
            "missing guard fails loud, never silent host write"
        );
        assert!(read(&probe).is_err(), "missing guard fails loud");
        assert!(!probe.exists(), "failed host write leaves no host entry");
    }

    #[test]
    fn parallel_guards_hold_separate_roots() {
        use std::sync::{Arc, Barrier};

        let barrier = Arc::new(Barrier::new(2));
        let spawn = |bytes: &'static [u8], barrier: Arc<Barrier>| {
            std::thread::spawn(move || {
                let _guard = TestGuard::install();
                let relative = PathBuf::from("parallel-shared/bytes.bin");
                let parent = relative.parent().unwrap().to_path_buf();
                create_dir_all(&parent).unwrap();
                write(&relative, bytes).unwrap();
                barrier.wait();
                let found = read(&relative).unwrap();
                assert_eq!(found, bytes.to_vec(), "separate roots keep own bytes");
            })
        };
        let first = spawn(b"alpha", barrier.clone());
        let second = spawn(b"beta", barrier);
        first.join().unwrap();
        second.join().unwrap();
        assert!(
            !PathBuf::from("parallel-shared/bytes.bin").exists(),
            "parallel writes never land on host"
        );
    }

    #[test]
    fn remove_dir_all_clears_nested_folders() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("tree").join("sub");
        create_dir_all(&nested).unwrap();
        let file = nested.join("note.bin");
        write(&file, b"nested").unwrap();
        assert!(exists(&file));
        remove_dir_all(&dir.path().join("tree")).unwrap();
        assert!(!exists(&file));
        assert!(!exists(&nested));
        assert!(
            !dir.path().join("tree").exists(),
            "guarded removal never lands on host"
        );
    }

    #[test]
    fn write_link_creates_resolvable_link() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("links").join("target.txt");
        create_dir_all(target.parent().unwrap()).unwrap();
        write(&target, b"linked bytes").unwrap();
        let link = dir.path().join("links").join("note");
        write_link(&link, std::path::Path::new("target.txt")).unwrap();
        assert_eq!(
            read_link(&link).unwrap(),
            PathBuf::from("target.txt"),
            "link reads back its target"
        );
        assert!(is_link(&link), "link reports as a link");
        assert!(!is_link(&target), "plain file reports no link");
        assert_eq!(
            read(&link).unwrap(),
            b"linked bytes".to_vec(),
            "reads follow the link"
        );
        let facts = metadata(&link).unwrap();
        assert_eq!(facts.len(), 12, "link facts follow the target");
        assert!(exists(&link), "live link counts as present");
        let moved = PathBuf::from("moved/target.txt");
        write_link(&link, &moved).unwrap();
        assert_eq!(
            read_link(&link).unwrap(),
            moved,
            "repeat link replaces the entry"
        );
        assert!(
            !link.exists(),
            "guarded link never lands on host: {}",
            link.display()
        );
    }

    #[test]
    fn canonicalize_follows_inside_link_and_marks_outside_by_structure() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        let target = root.join("target.txt");
        create_dir_all(&root).unwrap();
        write(&target, b"inside").unwrap();
        let inside = root.join("inside");
        write_link(&inside, std::path::Path::new("target.txt")).unwrap();
        let resolved = canonicalize(&inside).unwrap();
        assert_eq!(resolved, target, "inside link resolves at its target");
        assert!(
            resolved.starts_with(&root),
            "inside link stays under the root"
        );
        let outside = root.join("outside");
        let escaped = dir.path().join("escaped.txt");
        write(&escaped, b"outside").unwrap();
        write_link(&outside, &escaped).unwrap();
        let resolved = canonicalize(&outside).unwrap();
        assert!(
            !resolved.starts_with(&root),
            "outside link resolves outside the root by structure"
        );
    }

    #[test]
    fn canonicalize_fails_closed_on_link_loops() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        write_link(&first, &second).unwrap();
        write_link(&second, &first).unwrap();
        assert!(
            canonicalize(&first).is_err(),
            "loops fail closed under the guard"
        );
        assert!(read_link(&first).is_ok(), "loop links still read back");
        assert!(!exists(&first), "loop links count as absent");
        assert!(
            metadata(&first).is_err(),
            "loop facts fail closed under the guard"
        );
    }

    #[test]
    fn mode_reads_stored_bits_after_set_mode() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mode").join("note.bin");
        create_dir_all(file.parent().unwrap()).unwrap();
        write(&file, b"bits").unwrap();
        assert_eq!(
            mode(&file).unwrap(),
            0o644,
            "present entries without stored bits use the default"
        );
        set_mode(&file, 0o755).unwrap();
        assert_eq!(
            mode(&file).unwrap(),
            0o755,
            "guarded mode is the stored bits"
        );
        let missing = dir.path().join("mode").join("absent.bin");
        assert!(mode(&missing).is_err(), "absent mode fails loud");
        assert!(
            set_mode(&missing, 0o644).is_ok(),
            "guarded set always succeeds"
        );
    }

    #[test]
    fn guard_exists_seeds_plus_absent() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("seeded.bin");
        create_dir_all(file.parent().unwrap()).unwrap();
        write(&file, b"seed").unwrap();
        assert!(exists(&file), "guarded write is present");
        assert!(
            !exists(&dir.path().join("absent.bin")),
            "never-written path is absent"
        );
        assert!(
            !file.exists(),
            "guarded entries never land on host: {}",
            file.display()
        );
    }

    #[test]
    fn guard_mode_map_resets_on_install() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("reset").join("tool");
        {
            let _guard = TestGuard::install();
            create_dir_all(file.parent().unwrap()).unwrap();
            write(&file, b"run").unwrap();
            set_mode(&file, 0o755).unwrap();
            assert_eq!(mode(&file).unwrap(), 0o755);
        }
        {
            let _guard = TestGuard::install();
            create_dir_all(file.parent().unwrap()).unwrap();
            write(&file, b"run").unwrap();
            assert_eq!(
                mode(&file).unwrap(),
                0o644,
                "fresh guard drops stored bits back to the default"
            );
        }
    }

    #[test]
    fn guard_facts_match_host_disk_row_by_row() {
        use std::os::unix::fs::PermissionsExt as _;
        use std::path::Path;

        const EXEC_BIT: u32 = 0o111;

        /// Host verdict replaying the removed host rule on disk.
        fn host_hit(path: &PathBuf) -> bool {
            if !path.exists() {
                return false;
            }
            match std::fs::symlink_metadata(path) {
                Ok(facts) if facts.file_type().is_symlink() => true,
                Ok(facts) => facts.permissions().mode() & EXEC_BIT != 0,
                Err(_) => true,
            }
        }

        /// Guard verdict replaying the helper rule over driver facts.
        fn guard_hit(path: &Path) -> bool {
            if !exists(path) {
                return false;
            }
            match mode(path) {
                Ok(bits) if bits & EXEC_BIT != 0 => true,
                Ok(_) => false,
                Err(_) => true,
            }
        }

        let dir = tempfile::tempdir().unwrap();
        let tool = dir.path().join("parity-tool");
        let regular = dir.path().join("parity-regular");
        let link_ok = dir.path().join("parity-link-ok");
        let link_dead = dir.path().join("parity-link-dead");
        let absent = dir.path().join("parity-absent");

        std::fs::write(&tool, b"run").unwrap();
        std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(&regular, b"run").unwrap();
        std::fs::set_permissions(&regular, std::fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&tool, &link_ok).unwrap();
        std::os::unix::fs::symlink(dir.path().join("dangling-target"), &link_dead).unwrap();

        let _guard = TestGuard::install();
        create_dir_all(dir.path()).unwrap();
        write(&tool, b"run").unwrap();
        set_mode(&tool, 0o755).unwrap();
        write(&regular, b"run").unwrap();
        set_mode(&regular, 0o644).unwrap();
        // Current links mirror as stored exec entries, dangling links
        // mirror as missing paths, both reading the
        // same verdict the disk link reports.
        write_link(&link_ok, &tool).unwrap();
        set_mode(&link_ok, 0o755).unwrap();

        for (path, want_hit) in [
            (&tool, true),
            (&regular, false),
            (&link_ok, true),
            (&link_dead, false),
            (&absent, false),
        ] {
            assert_eq!(
                exists(path),
                path.exists(),
                "exists parity for {}",
                path.display()
            );
            assert_eq!(
                guard_hit(path),
                host_hit(path),
                "exec parity for {}",
                path.display()
            );
            assert_eq!(
                guard_hit(path),
                want_hit,
                "exec verdict for {}",
                path.display()
            );
        }
    }
}
