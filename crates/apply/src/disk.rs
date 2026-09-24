//! Disk backend behind snapshots, writes, and removals.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use confit_core::document::{ManifestData, ManifestDocument, ManifestMember};
use confit_core::error::{Error, Result};
use confit_core::handles::{BlobHandle, Route};

use crate::Applier;
use crate::resolve::{resolve_host, resolve_memory};
use crate::snapshot::{Snapshot, TreeMemberSnapshot, drain, snapshot_doc_host, snapshot_tree_host};

/// Disk backend selecting the host filesystem or the memory map.
///
/// Host reads live destinations through the environment.
/// Memory reads seeded paths under stable fake folders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskKind {
    /// Live host filesystem behind environment folders.
    Host,
    /// Seeded memory map behind fake folders.
    Memory,
}

/// Disk reads and writes behind one applier run.
pub trait Disk: Send + Sync {
    /// Expands one destination route to its backend path.
    fn resolve(&self, route: &Route) -> PathBuf;

    /// Snapshots one document destination through its kind-aware reader.
    fn snapshot_doc(&self, document: &ManifestDocument) -> Snapshot;

    /// Snapshots one tree destination into relative member readers.
    fn snapshot_tree(&self, document: &ManifestDocument) -> BTreeMap<String, TreeMemberSnapshot>;

    /// Opens a fresh streaming reader for one tree member path.
    fn open_member(&self, destination: &Route, relative: &str) -> Option<Box<dyn std::io::Read>>;

    /// Reports whether one backend path reads present.
    fn exists(&self, path: &Path) -> bool;

    /// Seeds one file behind a resolved path for memory runs.
    fn insert(&self, path: &Path, bytes: &[u8]);

    /// Writes bytes to one path, creating parents as needed.
    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()>;

    /// Streams one blob to its destination through the blob pool.
    fn write_blob(
        &self,
        dest: &Path,
        handle: &BlobHandle,
        blobs: &dyn confit_store::blob::BlobStore,
    ) -> Result<()>;

    /// Writes one tree destination member by member.
    fn write_tree(
        &self,
        dest: &Path,
        members: &[ManifestMember],
        blobs: &dyn confit_store::blob::BlobStore,
    ) -> Result<usize>;

    /// Creates one symlink, replacing present files.
    fn write_link(&self, link: &Path, target: &Path) -> std::io::Result<()>;

    /// Clears one symlink standing where a file lands.
    fn clear_link(&self, path: &Path) -> std::io::Result<()>;

    /// Sets unix permission bits on one path.
    fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()>;

    /// Removes one backend path, reporting whether anything left.
    fn remove(&self, path: &Path) -> std::io::Result<bool>;
}

/// Live host filesystem behind environment folders.
pub struct HostDisk;

impl Disk for HostDisk {
    fn resolve(&self, route: &Route) -> PathBuf {
        resolve_host(route)
    }

    fn snapshot_doc(&self, document: &ManifestDocument) -> Snapshot {
        let expanded = self.resolve(&document.destination);
        let is_link = matches!(document.data, ManifestData::Link { .. });
        snapshot_doc_host(&expanded, is_link)
    }

    fn snapshot_tree(&self, document: &ManifestDocument) -> BTreeMap<String, TreeMemberSnapshot> {
        let dir = self.resolve(&document.destination);
        snapshot_tree_host(&dir)
    }

    fn open_member(&self, destination: &Route, relative: &str) -> Option<Box<dyn std::io::Read>> {
        let dest = self.resolve(destination);
        let path = dest.join(relative);
        if let Ok(target) = std::fs::read_link(&path) {
            return Some(Box::new(std::io::Cursor::new(
                target.as_os_str().as_encoded_bytes().to_vec(),
            )) as Box<dyn std::io::Read>);
        }
        std::fs::File::open(&path)
            .ok()
            .map(|file| Box::new(file) as Box<dyn std::io::Read>)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn insert(&self, path: &Path, bytes: &[u8]) {
        let _ = self.write_bytes(path, bytes);
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        ensure_parent(path)?;
        std::fs::write(path, bytes)
    }

    fn write_blob(
        &self,
        dest: &Path,
        handle: &BlobHandle,
        blobs: &dyn confit_store::blob::BlobStore,
    ) -> Result<()> {
        copy_blob(dest, handle, blobs)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
    }

    fn write_tree(
        &self,
        dest: &Path,
        members: &[ManifestMember],
        blobs: &dyn confit_store::blob::BlobStore,
    ) -> Result<usize> {
        for member in members {
            let path = dest.join(&member.relative);
            copy_blob(&path, &member.blob, blobs).map_err(|error| match error {
                Error::Plan(_) => Error::Plan(format!(
                    "cannot write '{}': missing blob '{}' for '{}'",
                    dest.display(),
                    member.blob.sha(),
                    path.display()
                )),
                Error::Io(error) => Error::Plan(format!(
                    "cannot write '{}': cannot write '{}': {error}",
                    dest.display(),
                    path.display()
                )),
            })?;
            set_mode(&path, member.mode).map_err(|error| {
                Error::Plan(format!(
                    "cannot write '{}': cannot set mode '{}': {error}",
                    dest.display(),
                    path.display()
                ))
            })?;
        }
        Ok(1)
    }

    fn write_link(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        ensure_parent(link)?;
        if std::fs::symlink_metadata(link).is_ok() {
            std::fs::remove_file(link)?;
        }
        std::os::unix::fs::symlink(target, link)
    }

    fn clear_link(&self, path: &Path) -> std::io::Result<()> {
        if std::fs::read_link(path).is_ok() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }

    fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        set_mode(path, mode)
    }

    fn remove(&self, path: &Path) -> std::io::Result<bool> {
        if !path.exists() {
            return Ok(false);
        }
        std::fs::remove_file(path)?;
        Ok(true)
    }
}

/// Seeded memory map behind fake folders.
pub struct MemoryDisk {
    /// Holds seeded bytes behind resolved paths.
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

impl MemoryDisk {
    /// Empty memory map awaiting seeded files.
    pub(crate) fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    /// Locks the seeded map, recovering from poisoned reads.
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Vec<u8>>> {
        match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

impl Disk for MemoryDisk {
    fn resolve(&self, route: &Route) -> PathBuf {
        resolve_memory(route)
    }

    fn snapshot_doc(&self, document: &ManifestDocument) -> Snapshot {
        let expanded = self.resolve(&document.destination);
        let guard = self.lock();
        match guard.get(&expanded) {
            Some(bytes) => Snapshot::Present {
                reader: Box::new(std::io::Cursor::new(bytes.clone())),
                mode: None,
            },
            None => Snapshot::Absent,
        }
    }

    fn snapshot_tree(&self, document: &ManifestDocument) -> BTreeMap<String, TreeMemberSnapshot> {
        let dir = self.resolve(&document.destination);
        let guard = self.lock();
        let mut out = BTreeMap::new();
        for (file, bytes) in guard.iter() {
            let Ok(rel) = file.strip_prefix(&dir) else {
                continue;
            };
            let Some(name) = rel.to_str() else {
                continue;
            };
            out.insert(
                name.to_string(),
                TreeMemberSnapshot::Present {
                    reader: Box::new(std::io::Cursor::new(bytes.clone())),
                    mode: None,
                },
            );
        }
        out
    }

    fn open_member(&self, destination: &Route, relative: &str) -> Option<Box<dyn std::io::Read>> {
        let dest = self.resolve(destination);
        let path = dest.join(relative);
        let guard = self.lock();
        guard
            .get(&path)
            .cloned()
            .map(|bytes| Box::new(std::io::Cursor::new(bytes)) as Box<dyn std::io::Read>)
    }

    fn exists(&self, path: &Path) -> bool {
        self.lock().contains_key(path)
    }

    fn insert(&self, path: &Path, bytes: &[u8]) {
        self.lock().insert(path.to_path_buf(), bytes.to_vec());
    }

    fn write_bytes(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        self.lock().insert(path.to_path_buf(), bytes.to_vec());
        Ok(())
    }

    fn write_blob(
        &self,
        dest: &Path,
        handle: &BlobHandle,
        blobs: &dyn confit_store::blob::BlobStore,
    ) -> Result<()> {
        let bytes = read_blob(blobs, handle)?;
        self.lock().insert(dest.to_path_buf(), bytes);
        Ok(())
    }

    fn write_tree(
        &self,
        dest: &Path,
        members: &[ManifestMember],
        blobs: &dyn confit_store::blob::BlobStore,
    ) -> Result<usize> {
        for member in members {
            let bytes = read_blob(blobs, &member.blob)?;
            self.lock().insert(dest.join(&member.relative), bytes);
        }
        Ok(members.len())
    }

    fn write_link(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        self.lock().insert(
            link.to_path_buf(),
            target.as_os_str().as_encoded_bytes().to_vec(),
        );
        Ok(())
    }

    fn clear_link(&self, _path: &Path) -> std::io::Result<()> {
        Ok(())
    }

    fn set_mode(&self, _path: &Path, _mode: u32) -> std::io::Result<()> {
        Ok(())
    }

    fn remove(&self, path: &Path) -> std::io::Result<bool> {
        Ok(self.lock().remove(path).is_some())
    }
}

impl Applier {
    /// Seeds one file behind a resolved path for memory runs.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        self.disk.insert(path, bytes);
    }
}

/// Creates the parent folder for one destination path.
///
/// Empty parents skip.
///
/// # Errors
///
/// Missing ancestors and permission failures surface as io errors.
fn ensure_parent(dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Sets unix permission bits on one path.
///
/// # Errors
///
/// Missing paths and permission failures surface as io errors.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Streams one blob to its destination through the blob pool.
///
/// # Errors
///
/// Dangling hashes fail as plan errors naming the hash.
/// Unreadable sources and unwritable destinations fail as
/// plan or io errors.
fn copy_blob(
    dest: &Path,
    handle: &BlobHandle,
    blobs: &dyn confit_store::blob::BlobStore,
) -> Result<()> {
    use std::io::Write as _;

    let mut reader = blobs.open(handle)?;
    ensure_parent(dest).map_err(Error::from)?;
    let mut out = std::fs::File::create(dest).map_err(Error::from)?;
    std::io::copy(&mut reader, &mut out).map_err(Error::from)?;
    out.flush().map_err(Error::from)?;
    Ok(())
}

/// Streams one blob handle into bytes through the blob pool.
fn read_blob(blobs: &dyn confit_store::blob::BlobStore, handle: &BlobHandle) -> Result<Vec<u8>> {
    let mut reader = blobs.open(handle)?;
    drain(&mut reader)
        .map_err(|error| Error::Plan(format!("read blob '{}': {error}", handle.sha())))
}
