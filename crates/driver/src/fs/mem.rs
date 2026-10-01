//! Memory filesystem verbs behind `fs` paths.
//!
//! Every verb rides the parked backend under a test
//! guard with `FsFile` handles from open and create
//! and append. The backend splits reads and writes
//! across handles, so memory handles wrap cursors
//! flushing back on sync and drop.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};

use vfs::error::VfsErrorKind;
use vfs::{MemoryFS, VfsError, VfsFileType, VfsPath};

thread_local! {
    static PARKED: RefCell<Option<VfsPath>> = const { RefCell::new(None) };
    static MODES: RefCell<BTreeMap<String, u32>> = const { RefCell::new(BTreeMap::new()) };
    static READ_FAULT: RefCell<Option<io::ErrorKind>> = const { RefCell::new(None) };
    static WRITE_FAULT: RefCell<Option<io::ErrorKind>> = const { RefCell::new(None) };
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
        READ_FAULT.with(|cell| *cell.borrow_mut() = None);
        WRITE_FAULT.with(|cell| *cell.borrow_mut() = None);
        Self { prior }
    }

    /// Arms one sticky read failure for the guard life.
    ///
    /// Open with read with read_to_string fail until drop.
    pub fn fail_reads(&self, kind: io::ErrorKind) {
        READ_FAULT.with(|cell| *cell.borrow_mut() = Some(kind));
    }

    /// Arms one sticky write failure for the guard life.
    ///
    /// Write with create with append fail until drop.
    pub fn fail_writes(&self, kind: io::ErrorKind) {
        WRITE_FAULT.with(|cell| *cell.borrow_mut() = Some(kind));
    }
}

impl Drop for TestGuard {
    fn drop(&mut self) {
        PARKED.with(|cell| *cell.borrow_mut() = self.prior.take());
    }
}

/// Memory seekable handle over one cursor with a backend home.
///
/// Reads and seeks ride the cursor, writes buffer there,
/// length reads the cursor, sync and drop land bytes back
/// onto the backend.
struct MemFile {
    cursor: io::Cursor<Vec<u8>>,
    root: VfsPath,
    path: PathBuf,
}

impl io::Read for MemFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.cursor.read(buf)
    }
}

impl io::Write for MemFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.cursor.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.cursor.flush()
    }
}

impl io::Seek for MemFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.cursor.seek(pos)
    }
}

impl super::FsFile for MemFile {
    fn len(&self) -> io::Result<u64> {
        Ok(self.cursor.get_ref().len() as u64)
    }

    fn sync(&mut self) -> io::Result<()> {
        mem_write(&self.root, &self.path, self.cursor.get_ref())
    }
}

impl Drop for MemFile {
    fn drop(&mut self) {
        let bytes = self.cursor.get_ref().clone();
        let _ = mem_write(&self.root, &self.path, &bytes);
    }
}

/// Reads whole file bytes for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder and when no
///   guard holds.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    check_read()?;
    let root = rooted()?;
    mem_read(&root, path)
}

/// Reads whole file text for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [InvalidData] when bytes refuse UTF-8.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder and when no
///   guard holds.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    check_read()?;
    let root = rooted()?;
    mem_read_to_string(&root, path)
}

/// Writes whole file bytes to one path.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the parent holds no entry and when
///   no guard holds.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    check_write()?;
    let root = rooted()?;
    mem_write(&root, path, bytes)
}

/// Builds every missing folder along one path.
///
/// # Errors
///
/// - [AlreadyExists] when the path holds a file.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    let root = rooted()?;
    to_vfs(&root, path)?.create_dir_all().map_err(io_error)
}

/// Moves one file onto a new path overwriting any entry.
///
/// # Errors
///
/// - [NotFound] when the source holds no entry.
/// - [AlreadyExists] when a folder move meets a file.
/// - [InvalidInput] when path text refuses.
/// - [Other] when the destination parent holds no
///   entry and when no guard holds.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let root = rooted()?;
    mem_rename(&root, from, to)
}

/// Copies one file onto a new path overwriting any entry.
///
/// # Errors
///
/// - [NotFound] when the source holds no entry.
/// - [InvalidInput] when path text refuses.
/// - [Other] when the destination parent holds no
///   entry and when no guard holds.
pub fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    let root = rooted()?;
    let bytes = mem_read(&root, from)?;
    let len = bytes.len() as u64;
    mem_write(&root, to, &bytes)?;
    Ok(len)
}

/// Deletes one file.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn remove_file(path: &Path) -> io::Result<()> {
    let root = rooted()?;
    to_vfs(&root, path)?.remove_file().map_err(io_error)
}

/// Reads length and file-type facts for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
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
/// - [NotFound] when the folder holds no entry.
/// - [NotADirectory] when the path holds a file.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
    let root = rooted()?;
    mem_read_dir(&root, path)
}

/// Opens one file behind a seekable handle.
///
/// Reads ride the loaded bytes, writes buffer in the
/// cursor until sync or drop lands them back.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder and when no
///   guard holds.
pub fn open(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    check_read()?;
    let root = rooted()?;
    let bytes = mem_read(&root, path)?;
    let handle = MemFile {
        cursor: io::Cursor::new(bytes),
        root,
        path: path.to_path_buf(),
    };
    Ok(Box::new(handle) as Box<dyn super::FsFile>)
}

/// Reads the last bytes of one backend file.
///
/// Short backend files fail loud with unexpected-eof.
/// Missing files fail as not-found io errors.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [UnexpectedEof] when the file runs short of the tail.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
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

/// Creates one file behind a seekable handle truncating any entry.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the parent holds no entry and when
///   no guard holds.
pub fn create(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    check_write()?;
    let root = rooted()?;
    to_vfs(&root, path)?.create_file().map_err(io_error)?;
    let handle = MemFile {
        cursor: io::Cursor::new(Vec::new()),
        root,
        path: path.to_path_buf(),
    };
    Ok(Box::new(handle) as Box<dyn super::FsFile>)
}

/// Opens one file behind a seekable handle appending.
///
/// Present entries load first with the cursor at the
/// end, missing entries create first.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder, when the
///   parent holds no entry, and when no guard holds.
pub fn append(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    check_write()?;
    let root = rooted()?;
    let target = to_vfs(&root, path)?;
    let bytes = if target.exists().map_err(io_error)? {
        mem_read(&root, path)?
    } else {
        target.create_file().map_err(io_error)?;
        Vec::new()
    };
    let mut cursor = io::Cursor::new(bytes);
    cursor.seek(io::SeekFrom::End(0))?;
    let handle = MemFile {
        cursor,
        root,
        path: path.to_path_buf(),
    };
    Ok(Box::new(handle) as Box<dyn super::FsFile>)
}

/// Deletes one folder with every entry inside.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
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
/// - [NotFound] when the path holds any entry or none.
/// - [Other] when no guard holds.
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
/// - [AlreadyExists] when the parent holds a file.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
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
///
/// # Errors
///
/// - [Other] when no guard holds.
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
/// read the default. Missing entries fail loud.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
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

/// Fails one read with the armed kind while set.
///
/// Sticky reads fail until the guard drops.
fn check_read() -> io::Result<()> {
    if let Some(kind) = READ_FAULT.with(|cell| *cell.borrow()) {
        return Err(io::Error::new(kind, "confit driver: armed read fails"));
    }
    Ok(())
}

/// Fails one write with the armed kind while set.
///
/// Sticky writes fail until the guard drops.
fn check_write() -> io::Result<()> {
    if let Some(kind) = WRITE_FAULT.with(|cell| *cell.borrow()) {
        return Err(io::Error::new(kind, "confit driver: armed write fails"));
    }
    Ok(())
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
        VfsErrorKind::FileExists | VfsErrorKind::DirectoryExists => io::ErrorKind::AlreadyExists,
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

/// Reads backend length and file-type facts for one path.
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
