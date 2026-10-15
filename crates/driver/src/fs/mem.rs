//! Memory filesystem verbs.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{self, Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};

use vfs::error::VfsErrorKind;
use vfs::{MemoryFS, VfsError, VfsFileType, VfsPath};

thread_local! {
    static PARKED: RefCell<Option<VfsPath>> = const { RefCell::new(None) };
    static MODES: RefCell<BTreeMap<String, u32>> = const { RefCell::new(BTreeMap::new()) };
    static TICKS: RefCell<BTreeMap<String, u64>> = const { RefCell::new(BTreeMap::new()) };
    static LINKS: RefCell<BTreeMap<String, PathBuf>> = const { RefCell::new(BTreeMap::new()) };
    static CLOCK: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static READ_FAULT: RefCell<Option<io::ErrorKind>> = const { RefCell::new(None) };
    static WRITE_FAULT: RefCell<Option<io::ErrorKind>> = const { RefCell::new(None) };
}

/// RAII guard over one thread memory filesystem.
///
/// Install holds a fresh empty backend, drop restores the prior one.
pub struct TestGuard {
    prior: Option<VfsPath>,
}

impl TestGuard {
    /// Park a fresh empty memory filesystem for the thread.
    pub fn install() -> Self {
        let root = VfsPath::new(MemoryFS::new());
        let prior = PARKED.with(|cell| cell.borrow_mut().replace(root));
        MODES.with(|cell| cell.borrow_mut().clear());
        TICKS.with(|cell| cell.borrow_mut().clear());
        LINKS.with(|cell| cell.borrow_mut().clear());
        READ_FAULT.with(|cell| *cell.borrow_mut() = None);
        WRITE_FAULT.with(|cell| *cell.borrow_mut() = None);
        Self { prior }
    }

    /// Arm one sticky read failure for the guard life.
    ///
    /// - Open, read, and read_to_string fail until drop.
    pub fn fail_reads(&self, kind: io::ErrorKind) {
        READ_FAULT.with(|cell| *cell.borrow_mut() = Some(kind));
    }

    /// Arm one sticky write failure for the guard life.
    ///
    /// - Write, create, and append fail until drop.
    pub fn fail_writes(&self, kind: io::ErrorKind) {
        WRITE_FAULT.with(|cell| *cell.borrow_mut() = Some(kind));
    }
}

impl Drop for TestGuard {
    fn drop(&mut self) {
        PARKED.with(|cell| *cell.borrow_mut() = self.prior.take());
    }
}

/// Memory seekable handle.
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

/// Read whole file bytes for one path.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder and when no
///   guard holds.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    check_read()?;
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    mem_read(&root, &resolved)
}

/// Read whole file text for one path.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidData] when bytes refuse UTF-8.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder and when no
///   guard holds.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    check_read()?;
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    mem_read_to_string(&root, &resolved)
}

/// Write whole file bytes to one path.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] for missing parents and when
///   no guard holds.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    check_write()?;
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    mem_write(&root, &resolved, bytes)
}

/// Build every missing folder along one path.
///
/// # Errors
///
/// - [AlreadyExists] when the path holds a file.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    to_vfs(&root, &resolved)?.create_dir_all().map_err(io_error)
}

/// Move one file onto a new path and overwrite any entry.
///
/// # Errors
///
/// - [NotFound] for missing sources.
/// - [AlreadyExists] when a folder move meets a file.
/// - [InvalidInput] when path text refuses.
/// - [Other] for missing destination parents and when
///   no guard holds.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    let root = rooted()?;
    if shift_links(from, to) {
        remove_backend_file(&root, to);
        carry_ticks(from, to);
        return Ok(());
    }
    remove_link_entry(to);
    mem_rename(&root, from, to)?;
    carry_ticks(from, to);
    Ok(())
}

/// Carry write ticks across one rename.
fn carry_ticks(from: &Path, to: &Path) {
    let from_key = mode_key(from);
    let prefix = format!("{from_key}/");
    let base = mode_key(to);
    TICKS.with(|cell| {
        let mut ticks = cell.borrow_mut();
        let moved: Vec<(String, u64)> = ticks
            .iter()
            .filter(|(key, _)| *key == &from_key || key.starts_with(&prefix))
            .map(|(key, tick)| (key.clone(), *tick))
            .collect();
        for (key, tick) in moved {
            ticks.remove(&key);
            if key == from_key {
                ticks.insert(base.clone(), tick);
            } else {
                ticks.insert(format!("{base}{}", &key[prefix.len() - 1..]), tick);
            }
        }
    });
}

/// Copy one file onto a new path and overwrite any entry.
///
/// # Errors
///
/// - [NotFound] for missing sources.
/// - [InvalidInput] when path text refuses.
/// - [Other] for missing destination parents and when
///   no guard holds.
pub fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    let root = rooted()?;
    let source = resolve_path(from)?;
    let dest = resolve_path(to)?;
    let bytes = mem_read(&root, &source)?;
    let len = bytes.len() as u64;
    mem_write(&root, &dest, &bytes)?;
    Ok(len)
}

/// Delete one file.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn remove_file(path: &Path) -> io::Result<()> {
    let root = rooted()?;
    if remove_link_entry(path) {
        remove_backend_file(&root, path);
        return Ok(());
    }
    to_vfs(&root, path)?.remove_file().map_err(io_error)
}

/// Read length and file-type facts for one path.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn metadata(path: &Path) -> io::Result<super::FileMeta> {
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    mem_metadata(&root, &resolved)
}

/// Report whether one path holds any entry.
///
/// # Panics
///
/// Panic when no guard holds.
pub fn exists(path: &Path) -> bool {
    let Some(root) = parked() else {
        panic!("confit driver: no test guard installed");
    };
    let Ok(resolved) = resolve_path(path) else {
        return false;
    };
    mem_exists(&root, &resolved)
}

/// List immediate child paths under one folder sorted by name.
///
/// # Errors
///
/// - [NotFound] for missing folders.
/// - [NotADirectory] when the path holds a file.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    mem_read_dir(&root, &resolved, path)
}

/// Open one file.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder and when no
///   guard holds.
pub fn open(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    check_read()?;
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    let bytes = mem_read(&root, &resolved)?;
    let handle = MemFile {
        cursor: io::Cursor::new(bytes),
        root,
        path: resolved,
    };
    Ok(Box::new(handle) as Box<dyn super::FsFile>)
}

/// Read the last bytes of one backend file.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [UnexpectedEof] when the file runs short of the tail.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn read_tail(path: &Path, tail: u64) -> io::Result<Vec<u8>> {
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    let bytes = mem_read(&root, &resolved)?;
    let tail = tail as usize;
    if bytes.len() < tail {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("short file '{}'", path.display()),
        ));
    }
    Ok(bytes[bytes.len() - tail..].to_vec())
}

/// Create one file and truncate any entry.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] for missing parents and when
///   no guard holds.
pub fn create(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    check_write()?;
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    to_vfs(&root, &resolved)?.create_file().map_err(io_error)?;
    let handle = MemFile {
        cursor: io::Cursor::new(Vec::new()),
        root,
        path: resolved,
    };
    Ok(Box::new(handle) as Box<dyn super::FsFile>)
}

/// Open one file in append mode.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] when the path holds a folder, for missing parents,
///   and when no guard holds.
pub fn append(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    check_write()?;
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    let target = to_vfs(&root, &resolved)?;
    let bytes = if target.exists().map_err(io_error)? {
        mem_read(&root, &resolved)?
    } else {
        target.create_file().map_err(io_error)?;
        Vec::new()
    };
    let mut cursor = io::Cursor::new(bytes);
    cursor.seek(io::SeekFrom::End(0))?;
    let handle = MemFile {
        cursor,
        root,
        path: resolved,
    };
    Ok(Box::new(handle) as Box<dyn super::FsFile>)
}

/// Delete one folder with every entry inside.
///
/// # Errors
///
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    let root = rooted()?;
    if link_lookup(path).is_some() {
        remove_link_entry(path);
        remove_backend_file(&root, path);
        return Ok(());
    }
    let resolved = resolve_path(path)?;
    to_vfs(&root, &resolved)?
        .remove_dir_all()
        .map_err(io_error)?;
    remove_links_under(&resolved);
    Ok(())
}

/// Resolve one path through links to its absolute form.
///
/// - Relative inputs root at `/`, relative targets resolve at the link parent.
/// - Dangling and missing finals fail with [NotFound].
///
/// # Errors
///
/// - [NotFound] for missing resolved paths.
/// - [Other] when links loop and when no guard holds.
pub fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let root = rooted()?;
    let resolved = resolve_path(path)?;
    if mem_exists(&root, &resolved) {
        Ok(resolved)
    } else {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no entry '{}'", path.display()),
        ))
    }
}

/// Read one symlink target without following the final link.
///
/// - Parent links resolve before the final lookup.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidInput] for non-link paths.
/// - [Other] when parent links loop and when no guard holds.
pub fn read_link(path: &Path) -> io::Result<PathBuf> {
    let root = rooted()?;
    let candidate = link_candidate(path)?;
    if let Some(target) = link_lookup(&candidate) {
        return Ok(target);
    }
    if mem_exists(&root, &candidate) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("no link '{}'", path.display()),
        ));
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("no link '{}'", path.display()),
    ))
}

/// Create one symlink and replace any present entry.
///
/// - Relative targets stay relative to the link parent.
///
/// # Errors
///
/// - [AlreadyExists] when the path holds a folder.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when parent links loop and when no guard holds.
pub fn write_link(link: &Path, target: &Path) -> io::Result<()> {
    if let Some(parent) = link.parent() {
        create_dir_all(parent)?;
    }
    let root = rooted()?;
    let stored = link_candidate(link)?;
    if to_vfs(&root, &stored)
        .and_then(|entry| entry.is_dir().map_err(io_error))
        .unwrap_or(false)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("folder holds '{}'", link.display()),
        ));
    }
    remove_link_entry(&stored);
    remove_backend_file(&root, &stored);
    LINKS.with(|cell| {
        cell.borrow_mut()
            .insert(link_key(&stored), target.to_path_buf());
    });
    Ok(())
}

/// Set unix permission bits on one path.
///
/// - Present and absent paths alike succeed.
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

/// Read unix permission bits for one path.
///
/// - Stored bits win over the default.
///
/// # Errors
///
/// - [NotFound] for missing paths.
/// - [InvalidInput] when the path text refuses.
/// - [Other] when no guard holds.
pub fn mode(path: &Path) -> io::Result<u32> {
    let root = rooted()?;
    if link_lookup(path).is_some() {
        let stored = MODES.with(|cell| cell.borrow().get(&mode_key(path)).copied());
        return Ok(stored.unwrap_or(0o777));
    }
    let resolved = resolve_path(path)?;
    let target = to_vfs(&root, &resolved)?;
    if !target.exists().map_err(io_error)? {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("missing '{}'", path.display()),
        ));
    }
    let stored = MODES.with(|cell| cell.borrow().get(&mode_key(path)).copied());
    Ok(stored.unwrap_or(0o644))
}

/// Key one path for the side mode map under the guard.
fn mode_key(path: &Path) -> String {
    rooted_path(&path.as_os_str().to_string_lossy())
}

/// Clone the parked memory root while one guard holds it.
fn parked() -> Option<VfsPath> {
    PARKED.with(|cell| cell.borrow().clone())
}

/// Return the parked memory root. Require a guard for the call.
fn rooted() -> io::Result<VfsPath> {
    parked().ok_or_else(|| io::Error::other("confit driver: no test guard installed"))
}

/// Fail one read with the armed kind while set.
fn check_read() -> io::Result<()> {
    if let Some(kind) = READ_FAULT.with(|cell| *cell.borrow()) {
        return Err(io::Error::new(kind, "confit driver: armed read fails"));
    }
    Ok(())
}

/// Fail one write with the armed kind while set.
fn check_write() -> io::Result<()> {
    if let Some(kind) = WRITE_FAULT.with(|cell| *cell.borrow()) {
        return Err(io::Error::new(kind, "confit driver: armed write fails"));
    }
    Ok(())
}

/// Map one host path onto the parked memory backend.
///
/// - Absolute paths keep their text, relative paths root at `/`.
fn to_vfs(root: &VfsPath, path: &Path) -> io::Result<VfsPath> {
    let text = path.as_os_str().to_string_lossy();
    let rooted = rooted_path(&text);
    root.join(rooted.as_str()).map_err(io_error)
}

/// Root one path text at `/` for the memory backend.
fn rooted_path(text: &str) -> String {
    if text.starts_with('/') {
        text.to_string()
    } else {
        format!("/{text}")
    }
}

/// Map one backend failure onto an io failure and keep not-found.
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

/// Read whole file bytes from the parked memory backend.
fn mem_read(root: &VfsPath, path: &Path) -> io::Result<Vec<u8>> {
    let target = to_vfs(root, path)?;
    let mut opened = target.open_file().map_err(io_error)?;
    let mut bytes = Vec::new();
    opened.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Read whole file text from the parked memory backend.
fn mem_read_to_string(root: &VfsPath, path: &Path) -> io::Result<String> {
    to_vfs(root, path)?.read_to_string().map_err(io_error)
}

/// Write whole file bytes onto the parked memory backend.
fn mem_write(root: &VfsPath, path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut created = to_vfs(root, path)?.create_file().map_err(io_error)?;
    created.write_all(bytes)?;
    created.flush()?;
    touch(path);
    Ok(())
}

/// Bump the write tick for one path.
fn touch(path: &Path) {
    CLOCK.with(|clock| {
        let tick = clock.get().wrapping_add(1).max(1);
        clock.set(tick);
        TICKS.with(|cell| cell.borrow_mut().insert(mode_key(path), tick));
    });
}

/// Move one backend file onto a new path and overwrite any entry.
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

/// Move one backend folder onto a new path entry by entry.
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

/// Read backend length and file-type facts for one path.
fn mem_metadata(root: &VfsPath, path: &Path) -> io::Result<super::FileMeta> {
    let facts = to_vfs(root, path)?.metadata().map_err(io_error)?;
    // Side maps share one key form.
    let modified = TICKS.with(|cell| cell.borrow().get(&mode_key(path)).copied().unwrap_or(0));
    Ok(super::FileMeta::new(
        facts.len,
        facts.file_type == VfsFileType::Directory,
        modified,
    ))
}

/// Report whether one backend path holds any entry.
fn mem_exists(root: &VfsPath, path: &Path) -> bool {
    to_vfs(root, path)
        .and_then(|target| target.exists().map_err(io_error))
        .unwrap_or(false)
}

/// List sorted backend child paths under one folder.
fn mem_read_dir(root: &VfsPath, resolved: &Path, display: &Path) -> io::Result<Vec<PathBuf>> {
    let target = to_vfs(root, resolved)?;
    if target.is_file().map_err(io_error)? {
        return Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("not a directory '{}'", display.display()),
        ));
    }
    let mut entries = Vec::new();
    for entry in target.read_dir().map_err(io_error)? {
        entries.push(display.join(entry.filename()));
    }
    for name in link_children(resolved) {
        let shown = display.join(name);
        if !entries.contains(&shown) {
            entries.push(shown);
        }
    }
    entries.sort();
    Ok(entries)
}

/// Reduce one path lexically without touching entries.
fn lexical_normal(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(part) => out.push(part.as_os_str()),
            Component::RootDir => out.push("/"),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(part) => out.push(part),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Key one path for the link table under the guard.
fn link_key(path: &Path) -> String {
    lexical_normal(Path::new(&rooted_path(&path.as_os_str().to_string_lossy())))
        .to_string_lossy()
        .into_owned()
}

/// Copy one link target while the table holds it.
fn link_lookup(path: &Path) -> Option<PathBuf> {
    let key = link_key(path);
    LINKS.with(|cell| cell.borrow().get(&key).cloned())
}

/// Drop one link entry and report whether one held.
fn remove_link_entry(path: &Path) -> bool {
    let key = link_key(path);
    LINKS.with(|cell| cell.borrow_mut().remove(&key).is_some())
}

/// Drop one backend file while present and keep missing quiet.
fn remove_backend_file(root: &VfsPath, path: &Path) {
    if let Ok(entry) = to_vfs(root, path) {
        let _ = entry.remove_file();
    }
}

/// Drop every link under one resolved folder.
fn remove_links_under(resolved: &Path) {
    let prefix = format!("{}/", link_key(resolved));
    LINKS.with(|cell| {
        let dead: Vec<String> = cell
            .borrow()
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect();
        for key in dead {
            cell.borrow_mut().remove(&key);
        }
    });
}

/// List immediate link child names under one resolved folder.
fn link_children(resolved: &Path) -> Vec<PathBuf> {
    let base = link_key(resolved);
    let prefix = format!("{base}/");
    let mut names = Vec::new();
    LINKS.with(|cell| {
        for key in cell.borrow().keys() {
            if let Some(rest) = key.strip_prefix(&prefix)
                && !rest.contains('/')
                && !rest.is_empty()
            {
                names.push(PathBuf::from(rest));
            }
        }
    });
    names
}

/// Fold one path parent through links onto its stored form.
fn link_candidate(path: &Path) -> io::Result<PathBuf> {
    let Some(parent) = path.parent() else {
        return Ok(lexical_normal(path));
    };
    if parent.as_os_str().is_empty() {
        return Ok(lexical_normal(&PathBuf::from("/").join(path)));
    }
    if path.file_name().is_none() {
        return resolve_path(path);
    }
    let base = resolve_path(parent)?;
    Ok(base.join(path.file_name().unwrap_or_default()))
}

/// Move link entries under one rename and report an exact link move.
fn shift_links(from: &Path, to: &Path) -> bool {
    let from_key = link_key(from);
    let to_key = link_key(to);
    let prefix = format!("{from_key}/");
    let moved: Vec<(String, PathBuf)> = LINKS.with(|cell| {
        cell.borrow()
            .iter()
            .filter(|(key, _)| *key == &from_key || key.starts_with(&prefix))
            .map(|(key, target)| (key.clone(), target.clone()))
            .collect()
    });
    if moved.is_empty() {
        return false;
    }
    let exact = moved.iter().any(|(key, _)| *key == from_key);
    LINKS.with(|cell| {
        let mut links = cell.borrow_mut();
        for (key, _) in &moved {
            links.remove(key);
        }
        for (key, target) in moved {
            let next = if key == from_key {
                to_key.clone()
            } else {
                format!("{to_key}{}", &key[from_key.len()..])
            };
            links.insert(next, target);
        }
    });
    exact
}

/// Resolve every link along one path onto its absolute form.
fn resolve_path(path: &Path) -> io::Result<PathBuf> {
    let start = if path.is_absolute() {
        lexical_normal(path)
    } else {
        lexical_normal(&PathBuf::from("/").join(path))
    };
    let mut current = start;
    for _ in 0..40 {
        let Some((link, target, suffix)) = first_link(&current) else {
            return Ok(current);
        };
        current = if target.is_absolute() {
            lexical_normal(&target.join(&suffix))
        } else {
            let parent = link
                .parent()
                .map_or(PathBuf::from("/"), |held| held.to_path_buf());
            lexical_normal(&parent.join(&target).join(&suffix))
        };
    }
    Err(io::Error::other(format!(
        "symlink loop '{}'",
        path.display()
    )))
}

/// Find the first link prefix along one normalized absolute path.
fn first_link(current: &Path) -> Option<(PathBuf, PathBuf, PathBuf)> {
    use std::path::Component;

    let mut parts = Vec::new();
    for component in current.components() {
        if let Component::Normal(part) = component {
            parts.push(part.to_os_string());
        }
    }
    if let Some(target) = LINKS.with(|cell| cell.borrow().get("/").cloned()) {
        let mut suffix = PathBuf::new();
        for part in &parts {
            suffix.push(part);
        }
        return Some((PathBuf::from("/"), target, suffix));
    }
    let mut prefix = PathBuf::from("/");
    for (index, part) in parts.iter().enumerate() {
        prefix.push(part);
        let hit = LINKS.with(|cell| cell.borrow().get(&link_key(&prefix)).cloned());
        if let Some(target) = hit {
            let mut suffix = PathBuf::new();
            for rest in &parts[index + 1..] {
                suffix.push(rest);
            }
            return Some((prefix, target, suffix));
        }
    }
    None
}
