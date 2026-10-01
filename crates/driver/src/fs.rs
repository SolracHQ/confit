//! Filesystem verbs over host files with a memory backend under tests.
//!
//! Every verb rides `fs` paths with `FsFile` handles from
//! open and create and append. Host files wrap `std` files,
//! memory files wrap cursors flushing back on sync and drop.

use std::io;

#[cfg(not(any(test, feature = "test-support")))]
mod host;
#[cfg(not(any(test, feature = "test-support")))]
pub use host::*;

#[cfg(any(test, feature = "test-support"))]
mod mem;
#[cfg(any(test, feature = "test-support"))]
pub use mem::*;

/// Filesystem facts for one path.
///
/// Length holds content bytes, zero for folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    len: u64,
    is_dir: bool,
}

impl FileMeta {
    /// Builds facts from a byte length and the folder flag.
    pub fn new(len: u64, is_dir: bool) -> Self {
        Self { len, is_dir }
    }

    /// Byte length of one file, zero for folders.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether the length holds no bytes.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Whether the path holds a folder.
    pub fn is_dir(&self) -> bool {
        self.is_dir
    }

    /// Whether the path holds a plain file.
    pub fn is_file(&self) -> bool {
        !self.is_dir
    }
}

/// Seekable file handle shared by host and memory backends.
///
/// Handles answer reads and writes and seeks over one
/// cursor. Length reports committed bytes, sync lands
/// buffered bytes onto the backend.
#[allow(clippy::len_without_is_empty)]
pub trait FsFile: io::Read + io::Write + io::Seek {
    /// Reads the committed byte length of one file.
    ///
    /// Handles ride open files so missing never fires.
    /// Memory reads never fail, host faults surface OS
    /// kinds.
    ///
    /// # Errors
    ///
    /// - [Other] when host metadata faults.
    fn len(&self) -> io::Result<u64>;

    /// Lands buffered bytes onto the backend.
    ///
    /// Missing memory homes fail, host faults surface
    /// OS kinds.
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
        assert!(read(&probe).is_err(), "missing guard reads loud");
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
    fn write_link_stores_target_bytes_while_read_link_fails() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("links").join("note");
        let target = PathBuf::from("elsewhere/target.txt");
        write_link(&link, &target).unwrap();
        assert_eq!(
            read(&link).unwrap(),
            target.as_os_str().as_encoded_bytes(),
            "link stores target bytes as a plain file"
        );
        assert!(
            read_link(&link).is_err(),
            "link reads always fail under the guard"
        );
        let moved = PathBuf::from("moved/target.txt");
        write_link(&link, &moved).unwrap();
        assert_eq!(
            read(&link).unwrap(),
            moved.as_os_str().as_encoded_bytes(),
            "repeat link replaces the entry"
        );
        assert!(
            !link.exists(),
            "guarded link never lands on host: {}",
            link.display()
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
            "present entries without stored bits read the default"
        );
        set_mode(&file, 0o755).unwrap();
        assert_eq!(
            mode(&file).unwrap(),
            0o755,
            "guarded mode reads the stored bits"
        );
        let missing = dir.path().join("mode").join("absent.bin");
        assert!(mode(&missing).is_err(), "absent mode reads fail loud");
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
        assert!(exists(&file), "guarded write reads back present");
        assert!(
            !exists(&dir.path().join("absent.bin")),
            "never-written path reads absent"
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
        // Links hold no backend under the guard: a live link
        // mirrors as a stored exec entry, a dangling link
        // mirrors as a never-written path, both reading the
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
