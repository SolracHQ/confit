//! Driver
//!
//! `std::fs` shaped file access with a memory backend under tests.

#![deny(missing_docs)]

/// Filesystem facts for one path.
///
/// Length holds content bytes, zero for folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMeta {
    len: u64,
    is_dir: bool,
}

impl FileMeta {
    /// Builds facts from a byte length plus the folder flag.
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

pub use imp::*;

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
        let mut opened = open_read(&file).unwrap();
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
            std::io::Write::write_all(&mut out, b"+more").unwrap();
            std::io::Write::flush(&mut out).unwrap();
        }
        let moved = parent.join("moved.bin");
        rename(&stream, &moved).unwrap();
        assert!(!exists(&stream));
        assert_eq!(read(&moved).unwrap(), b"streamed+more".to_vec());
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

#[cfg(not(any(test, feature = "test-support")))]
mod imp {
    use std::io;
    use std::path::{Path, PathBuf};

    /// Reads whole file bytes for one path.
    ///
    /// # Errors
    ///
    /// Missing and unreadable files fail as io errors.
    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    /// Reads whole file text for one path.
    ///
    /// # Errors
    ///
    /// Missing, unreadable, and non-UTF-8 files fail as io errors.
    pub fn read_to_string(path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    /// Writes whole file bytes to one path.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        std::fs::write(path, bytes)
    }

    /// Builds every missing folder along one path.
    ///
    /// # Errors
    ///
    /// Unwritable folders fail as io errors.
    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    /// Moves one file onto a new path overwriting any entry.
    ///
    /// # Errors
    ///
    /// Missing sources and unwritable destinations fail as io errors.
    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        std::fs::rename(from, to)
    }

    /// Deletes one file.
    ///
    /// # Errors
    ///
    /// Missing files fail as io errors.
    pub fn remove_file(path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    /// Reads length plus file-type facts for one path.
    ///
    /// # Errors
    ///
    /// Missing paths fail as io errors.
    pub fn metadata(path: &Path) -> io::Result<super::FileMeta> {
        let facts = std::fs::metadata(path)?;
        Ok(super::FileMeta::new(
            facts.len(),
            facts.file_type().is_dir(),
        ))
    }

    /// Reports whether one path holds any entry.
    pub fn exists(path: &Path) -> bool {
        path.exists()
    }

    /// Lists immediate child paths under one folder sorted by name.
    ///
    /// # Errors
    ///
    /// Missing folders and files fail as io errors.
    pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path)? {
            entries.push(entry?.path());
        }
        entries.sort();
        Ok(entries)
    }

    /// Opens one file for streamed reading.
    ///
    /// # Errors
    ///
    /// Missing and unreadable files fail as io errors.
    pub fn open_read(path: &Path) -> io::Result<Box<dyn io::Read>> {
        std::fs::File::open(path).map(|file| Box::new(file) as Box<dyn io::Read>)
    }

    /// Reads the last bytes of one file seeking the tail.
    ///
    /// Short files fail loud instead of serving short
    /// reads. The pool footer reads this way, so blob
    /// lengths never load whole pool files.
    ///
    /// # Errors
    ///
    /// Missing files fail as not-found io errors. Files
    /// shorter than the tail fail as unexpected-eof io
    /// errors. Unreadable files fail as io errors.
    pub fn read_tail(path: &Path, tail: u64) -> io::Result<Vec<u8>> {
        use std::io::{Read as _, Seek as _};

        let facts = std::fs::metadata(path)?;
        if facts.len() < tail {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("short file '{}'", path.display()),
            ));
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(std::io::SeekFrom::End(-(tail as i64)))?;
        let mut out = vec![0u8; tail as usize];
        file.read_exact(&mut out)?;
        Ok(out)
    }

    /// Creates one file for streamed writing truncating any entry.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    pub fn create(path: &Path) -> io::Result<Box<dyn io::Write>> {
        std::fs::File::create(path).map(|file| Box::new(file) as Box<dyn io::Write>)
    }

    /// Opens one file for streamed appending creating any missing entry.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    pub fn append(path: &Path) -> io::Result<Box<dyn io::Write>> {
        std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .map(|file| Box::new(file) as Box<dyn io::Write>)
    }

    /// Deletes one folder with every entry inside.
    ///
    /// # Errors
    ///
    /// Missing folders fail as io errors.
    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    /// Reads one symlink target.
    ///
    /// # Errors
    ///
    /// Missing and non-link paths fail as io errors.
    pub fn read_link(path: &Path) -> io::Result<PathBuf> {
        std::fs::read_link(path)
    }

    /// Creates one symlink replacing any present entry.
    ///
    /// # Errors
    ///
    /// Unwritable folders fail as io errors.
    pub fn write_link(link: &Path, target: &Path) -> io::Result<()> {
        if let Some(parent) = link.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::symlink_metadata(link).is_ok() {
            std::fs::remove_file(link)?;
        }
        std::os::unix::fs::symlink(target, link)
    }

    /// Sets unix permission bits on one path.
    ///
    /// # Errors
    ///
    /// Missing paths and permission failures surface as io errors.
    pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
    }

    /// Reads unix permission bits for one path.
    ///
    /// # Errors
    ///
    /// Missing paths fail as io errors.
    pub fn mode(path: &Path) -> io::Result<u32> {
        use std::os::unix::fs::PermissionsExt;

        let facts = std::fs::symlink_metadata(path)?;
        Ok(facts.permissions().mode() & 0o777)
    }
}

#[cfg(any(test, feature = "test-support"))]
mod imp {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::io::{self, Read as _, Write as _};
    use std::path::{Path, PathBuf};

    use vfs::error::VfsErrorKind;
    use vfs::{MemoryFS, VfsError, VfsFileType, VfsPath};

    thread_local! {
        static PARKED: RefCell<Option<VfsPath>> = const { RefCell::new(None) };
        static MODES: RefCell<BTreeMap<String, u32>> = const { RefCell::new(BTreeMap::new()) };
    }

    /// RAII guard parking one memory filesystem for the thread.
    ///
    /// Install holds a fresh empty backend, drop restores the prior
    /// one so sequential tests on one thread never share entries.
    pub struct TestGuard {
        prior: Option<VfsPath>,
    }

    impl TestGuard {
        /// Parks a fresh empty memory filesystem for the thread.
        pub fn install() -> Self {
            let root = VfsPath::new(MemoryFS::new());
            let prior = PARKED.with(|cell| cell.borrow_mut().replace(root));
            MODES.with(|cell| cell.borrow_mut().clear());
            Self { prior }
        }
    }

    impl Drop for TestGuard {
        fn drop(&mut self) {
            PARKED.with(|cell| *cell.borrow_mut() = self.prior.take());
        }
    }

    /// Reads whole file bytes for one path.
    ///
    /// # Errors
    ///
    /// Missing and unreadable files fail as io errors.
    pub fn read(path: &Path) -> io::Result<Vec<u8>> {
        let root = rooted()?;
        mem_read(&root, path)
    }

    /// Reads whole file text for one path.
    ///
    /// # Errors
    ///
    /// Missing, unreadable, and non-UTF-8 files fail as io errors.
    pub fn read_to_string(path: &Path) -> io::Result<String> {
        let root = rooted()?;
        mem_read_to_string(&root, path)
    }

    /// Writes whole file bytes to one path.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let root = rooted()?;
        mem_write(&root, path, bytes)
    }

    /// Builds every missing folder along one path.
    ///
    /// # Errors
    ///
    /// Unwritable folders fail as io errors.
    pub fn create_dir_all(path: &Path) -> io::Result<()> {
        let root = rooted()?;
        to_vfs(&root, path)?.create_dir_all().map_err(io_error)
    }

    /// Moves one file onto a new path overwriting any entry.
    ///
    /// # Errors
    ///
    /// Missing sources and unwritable destinations fail as io errors.
    pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
        let root = rooted()?;
        mem_rename(&root, from, to)
    }

    /// Deletes one file.
    ///
    /// # Errors
    ///
    /// Missing files fail as io errors.
    pub fn remove_file(path: &Path) -> io::Result<()> {
        let root = rooted()?;
        to_vfs(&root, path)?.remove_file().map_err(io_error)
    }

    /// Reads length plus file-type facts for one path.
    ///
    /// # Errors
    ///
    /// Missing paths fail as io errors.
    pub fn metadata(path: &Path) -> io::Result<super::FileMeta> {
        let root = rooted()?;
        mem_metadata(&root, path)
    }

    /// Reports whether one path holds any entry.
    ///
    /// Panics without an installed guard: a bool carries
    /// no failure, and silent host reads are refused.
    pub fn exists(path: &Path) -> bool {
        let Some(root) = parked() else {
            panic!("confit driver: no test guard installed");
        };
        mem_exists(&root, path)
    }

    /// Lists immediate child paths under one folder sorted by name.
    ///
    /// # Errors
    ///
    /// Missing folders and files fail as io errors.
    pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
        let root = rooted()?;
        mem_read_dir(&root, path)
    }

    /// Opens one file for streamed reading.
    ///
    /// # Errors
    ///
    /// Missing and unreadable files fail as io errors.
    pub fn open_read(path: &Path) -> io::Result<Box<dyn io::Read>> {
        let root = rooted()?;
        mem_open_read(&root, path)
    }

    /// Reads the last bytes of one backend file.
    ///
    /// Short backend files fail loud with unexpected-eof.
    /// Missing files fail as not-found io errors.
    ///
    /// # Errors
    ///
    /// Missing files fail as not-found io errors. Files
    /// shorter than the tail fail as unexpected-eof io
    /// errors.
    pub fn read_tail(path: &Path, tail: u64) -> io::Result<Vec<u8>> {
        let root = rooted()?;
        let bytes = mem_read(&root, path)?;
        let tail = tail as usize;
        if bytes.len() < tail {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                format!("short file '{}'", path.display()),
            ));
        }
        Ok(bytes[bytes.len() - tail..].to_vec())
    }

    /// Creates one file for streamed writing truncating any entry.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    pub fn create(path: &Path) -> io::Result<Box<dyn io::Write>> {
        let root = rooted()?;
        mem_create(&root, path)
    }

    /// Opens one file for streamed appending creating any missing entry.
    ///
    /// # Errors
    ///
    /// Unwritable folders and files fail as io errors.
    pub fn append(path: &Path) -> io::Result<Box<dyn io::Write>> {
        let root = rooted()?;
        mem_append(&root, path)
    }

    /// Deletes one folder with every entry inside.
    ///
    /// # Errors
    ///
    /// Missing folders fail as io errors.
    pub fn remove_dir_all(path: &Path) -> io::Result<()> {
        let root = rooted()?;
        to_vfs(&root, path)?.remove_dir_all().map_err(io_error)
    }

    /// Reads one symlink target.
    ///
    /// Links never model in tests: every path reads as a
    /// plain file, so reads always fail and link policy
    /// stays pure logic.
    ///
    /// # Errors
    ///
    /// Every path fails as a not-found io error.
    pub fn read_link(path: &Path) -> io::Result<PathBuf> {
        let _ = rooted()?;
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no link '{}'", path.display()),
        ))
    }

    /// Creates one symlink replacing any present entry.
    ///
    /// Links store target bytes as a plain file, so
    /// readers see link content without link semantics.
    ///
    /// # Errors
    ///
    /// Unwritable folders fail as io errors.
    pub fn write_link(link: &Path, target: &Path) -> io::Result<()> {
        if let Some(parent) = link.parent() {
            create_dir_all(parent)?;
        }
        write(link, target.as_os_str().as_encoded_bytes())
    }

    /// Sets unix permission bits on one path.
    ///
    /// Bits persist beside the backend under the guard, so
    /// later reads see stored values. Present and absent
    /// paths alike succeed; absent reads still fail loud.
    pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
        let _ = rooted()?;
        MODES.with(|cell| {
            cell.borrow_mut().insert(mode_key(path), mode);
        });
        Ok(())
    }

    /// Reads unix permission bits for one path.
    ///
    /// Stored bits win, present entries without stored bits
    /// read the default. Missing entries fail as io errors.
    ///
    /// # Errors
    ///
    /// Missing paths fail as io errors.
    pub fn mode(path: &Path) -> io::Result<u32> {
        let root = rooted()?;
        let target = to_vfs(&root, path)?;
        if !target.exists().map_err(io_error)? {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("missing '{}'", path.display()),
            ));
        }
        let stored = MODES.with(|cell| cell.borrow().get(&mode_key(path)).copied());
        Ok(stored.unwrap_or(0o644))
    }

    /// Keys one path for the side mode map under the guard.
    fn mode_key(path: &Path) -> String {
        rooted_path(&path.as_os_str().to_string_lossy())
    }

    /// Clones the parked memory root while one guard holds it.
    fn parked() -> Option<VfsPath> {
        PARKED.with(|cell| cell.borrow().clone())
    }

    /// Reads the parked memory root failing loud without a guard.
    ///
    /// Tests forgetting the guard fail here, never on the host.
    fn rooted() -> io::Result<VfsPath> {
        parked().ok_or_else(|| io::Error::other("confit driver: no test guard installed"))
    }

    /// Maps one host path onto the parked memory backend.
    ///
    /// Absolute paths keep their text, relative paths root at `/`.
    fn to_vfs(root: &VfsPath, path: &Path) -> io::Result<VfsPath> {
        let text = path.as_os_str().to_string_lossy();
        let rooted = rooted_path(&text);
        root.join(rooted.as_str()).map_err(io_error)
    }

    /// Roots one path text at `/` for the memory backend.
    fn rooted_path(text: &str) -> String {
        if text.starts_with('/') {
            text.to_string()
        } else {
            format!("/{text}")
        }
    }

    /// Maps one backend failure onto an io failure keeping not-found.
    fn io_error(error: VfsError) -> io::Error {
        let kind = match error.kind() {
            VfsErrorKind::FileNotFound => io::ErrorKind::NotFound,
            VfsErrorKind::FileExists | VfsErrorKind::DirectoryExists => {
                io::ErrorKind::AlreadyExists
            }
            VfsErrorKind::InvalidPath => io::ErrorKind::InvalidInput,
            VfsErrorKind::IoError(inner) => {
                return io::Error::new(inner.kind(), format!("{error}"));
            }
            _ => io::ErrorKind::Other,
        };
        io::Error::new(kind, format!("{error}"))
    }

    /// Reads whole file bytes from the parked memory backend.
    fn mem_read(root: &VfsPath, path: &Path) -> io::Result<Vec<u8>> {
        let target = to_vfs(root, path)?;
        let mut opened = target.open_file().map_err(io_error)?;
        let mut bytes = Vec::new();
        opened.read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    /// Reads whole file text from the parked memory backend.
    fn mem_read_to_string(root: &VfsPath, path: &Path) -> io::Result<String> {
        to_vfs(root, path)?.read_to_string().map_err(io_error)
    }

    /// Writes whole file bytes onto the parked memory backend.
    fn mem_write(root: &VfsPath, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut created = to_vfs(root, path)?.create_file().map_err(io_error)?;
        created.write_all(bytes)?;
        created.flush()?;
        Ok(())
    }

    /// Moves one backend file onto a new path overwriting any entry.
    fn mem_rename(root: &VfsPath, from: &Path, to: &Path) -> io::Result<()> {
        let source = to_vfs(root, from)?;
        let dest = to_vfs(root, to)?;
        if source.is_dir().map_err(io_error)? {
            return mem_rename_dir(&source, &dest);
        }
        if dest.exists().map_err(io_error)? {
            dest.remove_file().map_err(io_error)?;
        }
        source.move_file(&dest).map_err(io_error)
    }

    /// Moves one backend folder onto a new path entry by entry.
    fn mem_rename_dir(source: &VfsPath, dest: &VfsPath) -> io::Result<()> {
        dest.create_dir_all().map_err(io_error)?;
        let names: Vec<String> = source
            .read_dir()
            .map_err(io_error)?
            .map(|entry| entry.filename())
            .collect();
        for name in names {
            let entry = source.join(name.as_str()).map_err(io_error)?;
            let target = dest.join(entry.filename().as_str()).map_err(io_error)?;
            if entry.is_dir().map_err(io_error)? {
                mem_rename_dir(&entry, &target)?;
            } else {
                entry.move_file(&target).map_err(io_error)?;
            }
        }
        source.remove_dir().map_err(io_error)
    }

    /// Reads backend length plus file-type facts for one path.
    fn mem_metadata(root: &VfsPath, path: &Path) -> io::Result<super::FileMeta> {
        let facts = to_vfs(root, path)?.metadata().map_err(io_error)?;
        Ok(super::FileMeta::new(
            facts.len,
            facts.file_type == VfsFileType::Directory,
        ))
    }

    /// Reports whether one backend path holds any entry.
    fn mem_exists(root: &VfsPath, path: &Path) -> bool {
        to_vfs(root, path)
            .and_then(|target| target.exists().map_err(io_error))
            .unwrap_or(false)
    }

    /// Lists sorted backend child paths under one folder.
    fn mem_read_dir(root: &VfsPath, path: &Path) -> io::Result<Vec<PathBuf>> {
        let target = to_vfs(root, path)?;
        if target.is_file().map_err(io_error)? {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!("not a directory '{}'", path.display()),
            ));
        }
        let mut entries = Vec::new();
        for entry in target.read_dir().map_err(io_error)? {
            entries.push(path.join(entry.filename()));
        }
        entries.sort();
        Ok(entries)
    }

    /// Opens one backend file for streamed reading.
    fn mem_open_read(root: &VfsPath, path: &Path) -> io::Result<Box<dyn io::Read>> {
        let bytes = mem_read(root, path)?;
        Ok(Box::new(io::Cursor::new(bytes)) as Box<dyn io::Read>)
    }

    /// Creates one backend file for streamed writing.
    fn mem_create(root: &VfsPath, path: &Path) -> io::Result<Box<dyn io::Write>> {
        let created = to_vfs(root, path)?.create_file().map_err(io_error)?;
        Ok(Box::new(created) as Box<dyn io::Write>)
    }

    /// Opens one backend file for streamed appending creating any missing entry.
    fn mem_append(root: &VfsPath, path: &Path) -> io::Result<Box<dyn io::Write>> {
        let target = to_vfs(root, path)?;
        if target.exists().map_err(io_error)? {
            let appended = target.append_file().map_err(io_error)?;
            return Ok(Box::new(appended) as Box<dyn io::Write>);
        }
        mem_create(root, path)
    }
}
