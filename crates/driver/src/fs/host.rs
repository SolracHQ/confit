//! Host filesystem verbs behind `fs` paths.
//!
//! Every verb rides `std` directly with `FsFile`
//! handles from open and create and append.

use std::io;
use std::path::{Path, PathBuf};

/// Host seekable handle over one `std` file.
///
/// Reads and seeks ride the file, length reads
/// metadata, sync lands through `sync_all`.
struct HostFile {
    file: std::fs::File,
}

impl io::Read for HostFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.file.read(buf)
    }
}

impl io::Write for HostFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.file.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

impl io::Seek for HostFile {
    fn seek(&mut self, pos: io::SeekFrom) -> io::Result<u64> {
        self.file.seek(pos)
    }
}

impl super::FsFile for HostFile {
    fn len(&self) -> io::Result<u64> {
        Ok(self.file.metadata()?.len())
    }

    fn sync(&mut self) -> io::Result<()> {
        self.file.sync_all()
    }
}

/// Reads whole file bytes for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    std::fs::read(path)
}

/// Reads whole file text for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
/// - [InvalidData] when bytes refuse UTF-8.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    std::fs::read_to_string(path)
}

/// Writes whole file bytes to one path.
///
/// # Errors
///
/// - [NotFound] when the parent holds no entry.
/// - [PermissionDenied] when the path refuses writes.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)
}

/// Builds every missing folder along one path.
///
/// # Errors
///
/// - [PermissionDenied] when folders refuse writes.
/// - [NotADirectory] when a file blocks the path.
pub fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Moves one file onto a new path overwriting any entry.
///
/// # Errors
///
/// - [NotFound] when the source holds no entry.
/// - [PermissionDenied] when the destination refuses writes.
/// - [InvalidInput] when a folder meets a file.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Copies one file onto a new path overwriting any entry.
///
/// # Errors
///
/// - [NotFound] when the source holds no entry.
/// - [PermissionDenied] when the destination refuses writes.
pub fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    std::fs::copy(from, to)
}

/// Deletes one file.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses removal.
pub fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Reads length and file-type facts for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
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
/// - [NotFound] when the folder holds no entry.
/// - [PermissionDenied] when the folder refuses reads.
/// - [NotADirectory] when the path holds a file.
pub fn read_dir(path: &Path) -> io::Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(path)? {
        entries.push(entry?.path());
    }
    entries.sort();
    Ok(entries)
}

/// Opens one file behind a seekable handle.
///
/// Reads and seeks ride the handle, writes refuse on
/// the read-only file.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
pub fn open(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    std::fs::File::open(path).map(|file| Box::new(HostFile { file }) as Box<dyn super::FsFile>)
}

/// Reads the last bytes of one file seeking the tail.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
/// - [UnexpectedEof] when the file runs short of the tail.
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
    file.seek(io::SeekFrom::End(-(tail as i64)))?;
    let mut out = vec![0u8; tail as usize];
    file.read_exact(&mut out)?;
    Ok(out)
}

/// Creates one file behind a seekable handle truncating any entry.
///
/// # Errors
///
/// - [NotFound] when the parent holds no entry.
/// - [PermissionDenied] when the path refuses writes.
pub fn create(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    std::fs::File::create(path).map(|file| Box::new(HostFile { file }) as Box<dyn super::FsFile>)
}

/// Opens one file behind a seekable handle appending.
///
/// Missing entries create first, writes land at the end.
///
/// # Errors
///
/// - [NotFound] when the parent holds no entry.
/// - [PermissionDenied] when the path refuses writes.
pub fn append(path: &Path) -> io::Result<Box<dyn super::FsFile>> {
    std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map(|file| Box::new(HostFile { file }) as Box<dyn super::FsFile>)
}

/// Deletes one folder with every entry inside.
///
/// # Errors
///
/// - [NotFound] when the folder holds no entry.
/// - [PermissionDenied] when the folder refuses removal.
/// - [NotADirectory] when the path holds a file.
pub fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Reads one symlink target.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
/// - [InvalidInput] when the path holds no link.
pub fn read_link(path: &Path) -> io::Result<PathBuf> {
    std::fs::read_link(path)
}

/// Creates one symlink replacing any present entry.
///
/// # Errors
///
/// - [NotFound] when the parent holds no entry.
/// - [PermissionDenied] when folders refuse writes.
/// - [InvalidInput] when the link path holds bad bytes.
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
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses writes.
pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Reads unix permission bits for one path.
///
/// # Errors
///
/// - [NotFound] when the path holds no entry.
/// - [PermissionDenied] when the path refuses reads.
pub fn mode(path: &Path) -> io::Result<u32> {
    use std::os::unix::fs::PermissionsExt;

    let facts = std::fs::symlink_metadata(path)?;
    Ok(facts.permissions().mode() & 0o777)
}
