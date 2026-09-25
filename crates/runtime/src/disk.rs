//! Disk backend behind live reads, writes, and removals.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_driver as driver;
use confit_model::document::{ManifestData, ManifestDocument, ManifestMember};
use confit_model::error::{Error, Result};
use confit_model::handles::{BlobHandle, Route};
use confit_store::blob::BlobStore;

use crate::Applier;
use crate::resolve::resolve_host;

/// Disk reads and writes behind one applier run.
///
/// Every call rides the driver: host paths in production,
/// memory under a test guard. Routes expand against host
/// folders in both cases, so isolation rides the guard.
pub struct HostDisk;

/// Live disk state behind one document destination.
///
/// Readers stream content without loading whole files.
/// Modes ride beside readers for permission comparison.
pub enum Live {
    /// Missing destination awaiting creation.
    Absent,
    /// Failing read carrying the raw failure detail.
    Unreadable {
        /// Holds the raw failure detail from the read.
        reason: String,
    },
    /// Content reader plus permission bits for the destination.
    Present {
        /// Streams destination bytes.
        reader: Box<dyn std::io::Read>,
        /// Holds unix permission bits, None for links.
        mode: Option<u32>,
    },
}

/// Live disk state behind one tree member.
///
/// Members absent from disk never enter the map, so absence
/// reads as a map miss and no `Absent` variant rides here.
pub enum LiveMember {
    /// Failing read carrying the raw failure detail.
    Unreadable {
        /// Holds the raw failure detail from the read.
        reason: String,
    },
    /// Content reader plus permission bits for the member.
    Present {
        /// Streams member bytes.
        reader: Box<dyn std::io::Read>,
        /// Holds unix permission bits, None for links.
        mode: Option<u32>,
    },
}

impl HostDisk {
    /// Expands one destination route to its backend path.
    pub fn resolve(&self, route: &Route) -> PathBuf {
        resolve_host(route)
    }

    /// Reads one document destination through its kind-aware reader.
    pub fn live_doc(&self, document: &ManifestDocument) -> Live {
        let expanded = self.resolve(&document.destination);
        let is_link = matches!(document.data, ManifestData::Link { .. });
        live_doc_host(&expanded, is_link)
    }

    /// Reads one tree destination into relative member readers.
    pub fn live_tree(&self, document: &ManifestDocument) -> BTreeMap<String, LiveMember> {
        let dir = self.resolve(&document.destination);
        live_tree_host(&dir)
    }

    /// Opens a fresh streaming reader for one tree member path.
    pub fn open_member(
        &self,
        destination: &Route,
        relative: &str,
    ) -> Option<Box<dyn std::io::Read>> {
        let dest = self.resolve(destination);
        let path = dest.join(relative);
        if let Ok(target) = driver::read_link(&path) {
            return Some(Box::new(std::io::Cursor::new(
                target.as_os_str().as_encoded_bytes().to_vec(),
            )) as Box<dyn std::io::Read>);
        }
        driver::open_read(&path).ok()
    }

    /// Reports whether one backend path reads present.
    pub fn exists(&self, path: &Path) -> bool {
        driver::exists(path)
    }

    /// Seeds one file behind a resolved path for memory runs.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        let _ = self.write_bytes(path, bytes);
    }

    /// Writes bytes to one path, creating parents as needed.
    pub fn write_bytes(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        ensure_parent(path)?;
        driver::write(path, bytes)
    }

    /// Streams one blob to its destination through the blob pool.
    pub fn write_blob(&self, dest: &Path, handle: &BlobHandle, blobs: &BlobStore) -> Result<()> {
        copy_blob(dest, handle, blobs)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
    }

    /// Writes one tree destination member by member.
    pub fn write_tree(
        &self,
        dest: &Path,
        members: &[ManifestMember],
        blobs: &BlobStore,
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
            driver::set_mode(&path, member.mode).map_err(|error| {
                Error::Plan(format!(
                    "cannot write '{}': cannot set mode '{}': {error}",
                    dest.display(),
                    path.display()
                ))
            })?;
        }
        Ok(1)
    }

    /// Creates one symlink, replacing present files.
    pub fn write_link(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        driver::write_link(link, target)
    }

    /// Clears one symlink standing where a file lands.
    pub fn clear_link(&self, path: &Path) -> std::io::Result<()> {
        if driver::read_link(path).is_ok() {
            driver::remove_file(path)?;
        }
        Ok(())
    }

    /// Sets unix permission bits on one path.
    pub fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        driver::set_mode(path, mode)
    }

    /// Removes one backend path, reporting whether anything left.
    pub fn remove(&self, path: &Path) -> std::io::Result<bool> {
        if !driver::exists(path) {
            return Ok(false);
        }
        driver::remove_file(path)?;
        Ok(true)
    }
}

impl Applier {
    /// Seeds one file behind a resolved path for memory runs.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        self.disk.insert(path, bytes);
    }
}

/// Reads one host document destination through its kind-aware reader.
fn live_doc_host(expanded: &Path, is_link: bool) -> Live {
    let target = match driver::read_link(expanded) {
        Ok(link) => join_link_target(expanded, &link),
        Err(_) => expanded.to_path_buf(),
    };
    if is_link {
        return match driver::read_link(expanded) {
            Ok(link) => Live::Present {
                reader: Box::new(std::io::Cursor::new(
                    link.as_os_str().as_encoded_bytes().to_vec(),
                )),
                mode: None,
            },
            Err(_) => match driver::open_read(expanded) {
                Ok(file) => Live::Present {
                    reader: Box::new(file),
                    mode: file_mode(expanded),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Live::Absent,
                Err(error) => Live::Unreadable {
                    reason: error.to_string(),
                },
            },
        };
    }
    match driver::open_read(&target) {
        Ok(file) => Live::Present {
            reader: Box::new(file),
            mode: file_mode(&target),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Live::Absent,
        Err(error) => Live::Unreadable {
            reason: error.to_string(),
        },
    }
}

/// Reads one host tree destination into relative member readers.
fn live_tree_host(dir: &Path) -> BTreeMap<String, LiveMember> {
    let mut out = BTreeMap::new();
    walk_tree(dir, dir, &mut out);
    out
}

/// Walks one folder into relative member readers.
///
/// Missing folders read empty.
fn walk_tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, LiveMember>) {
    let children = match driver::read_dir(dir) {
        Ok(children) => children,
        Err(_) => return,
    };
    for child in children {
        let rel = match child.strip_prefix(root) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => continue,
        };
        let Some(name) = rel.to_str() else {
            continue;
        };
        if let Ok(items) = driver::read_dir(&child)
            && !items.is_empty()
        {
            walk_tree(root, &child, out);
            continue;
        }
        if let Ok(target) = driver::read_link(&child) {
            out.insert(
                name.to_string(),
                LiveMember::Present {
                    reader: Box::new(std::io::Cursor::new(
                        target.as_os_str().as_encoded_bytes().to_vec(),
                    )),
                    mode: None,
                },
            );
            continue;
        }
        match driver::open_read(&child) {
            Ok(file) => {
                out.insert(
                    name.to_string(),
                    LiveMember::Present {
                        reader: Box::new(file),
                        mode: file_mode(&child),
                    },
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                out.insert(
                    name.to_string(),
                    LiveMember::Unreadable {
                        reason: error.to_string(),
                    },
                );
            }
        }
    }
}

/// Joins a link target against the link parent without touching disk.
///
/// Relative targets resolve beside the link. Parent segments pop
/// lexically, staying put past the root.
fn join_link_target(link: &Path, target: &Path) -> std::path::PathBuf {
    if target.is_absolute() {
        return target.to_path_buf();
    }
    let mut out = match link.parent() {
        Some(parent) => parent.to_path_buf(),
        None => std::path::PathBuf::new(),
    };
    for part in target.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out
}

/// Reads unix permission bits for one path.
///
/// Links read as None.
fn file_mode(path: &Path) -> Option<u32> {
    if driver::read_link(path).is_ok() {
        return None;
    }
    driver::mode(path).ok()
}

/// Drains one streaming reader into bytes chunk by chunk.
pub(crate) fn drain(reader: &mut dyn std::io::Read) -> std::io::Result<Vec<u8>> {
    const DRAIN_CHUNK: usize = 8192;

    let mut out = Vec::new();
    let mut chunk = [0u8; DRAIN_CHUNK];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            return Ok(out);
        }
        out.extend_from_slice(&chunk[..read]);
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
        driver::create_dir_all(parent)?;
    }
    Ok(())
}

/// Streams one blob to its destination through the blob pool.
///
/// # Errors
///
/// Dangling hashes fail as plan errors naming the hash.
/// Unreadable sources and unwritable destinations fail as
/// plan or io errors.
fn copy_blob(dest: &Path, handle: &BlobHandle, blobs: &BlobStore) -> Result<()> {
    use std::io::Write as _;

    let mut reader = blobs.open(handle)?;
    ensure_parent(dest).map_err(Error::from)?;
    let mut out = driver::create(dest).map_err(Error::from)?;
    std::io::copy(&mut reader, &mut out).map_err(Error::from)?;
    out.flush().map_err(Error::from)?;
    Ok(())
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use confit_driver::TestGuard;
    use confit_model::handles::{Route, RouteBase};

    fn literal(path: &Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &Path) -> ManifestDocument {
        ManifestDocument::new(
            literal(path),
            ManifestData::Text {
                content: "live bytes".into(),
                mode: None,
                unmanaged: false,
            },
        )
    }

    #[test]
    fn absent_doc_reads_absent() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let disk = HostDisk;
        match disk.live_doc(&text_doc(&dir.path().join("absent.txt"))) {
            Live::Absent => {}
            Live::Present { .. } => panic!("never-written path reads present"),
            Live::Unreadable { reason } => panic!("never-written path reads unreadable: {reason}"),
        }
    }

    #[test]
    fn present_doc_streams_bytes_plus_mode() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("present.txt");
        let disk = HostDisk;
        disk.write_bytes(&path, b"live bytes").unwrap();
        driver::set_mode(&path, 0o755).unwrap();
        match disk.live_doc(&text_doc(&path)) {
            Live::Present { mut reader, mode } => {
                assert_eq!(drain(&mut reader).unwrap(), b"live bytes".to_vec());
                assert_eq!(mode, Some(0o755), "live mode rides beside the reader");
            }
            Live::Absent => panic!("written path reads absent"),
            Live::Unreadable { reason } => panic!("written path reads unreadable: {reason}"),
        }
    }

    #[test]
    fn unreadable_doc_names_the_reason() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked.txt");
        driver::create_dir_all(&path).unwrap();
        let disk = HostDisk;
        match disk.live_doc(&text_doc(&path)) {
            Live::Unreadable { reason } => assert!(
                !reason.is_empty(),
                "unreadable carries the raw failure detail"
            ),
            Live::Absent => panic!("directory path reads absent"),
            Live::Present { .. } => panic!("directory path reads present"),
        }
    }

    #[test]
    fn member_absence_reads_as_map_miss() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tree");
        let disk = HostDisk;
        disk.write_bytes(&root.join("present.txt"), b"member bytes")
            .unwrap();
        let document = ManifestDocument::new(
            literal(&root),
            ManifestData::Tree {
                members: Vec::new(),
            },
        );
        let mut found = disk.live_tree(&document);
        match found.get_mut("present.txt") {
            Some(LiveMember::Present { reader, .. }) => {
                assert_eq!(drain(reader).unwrap(), b"member bytes".to_vec());
            }
            Some(LiveMember::Unreadable { reason }) => {
                panic!("written member reads unreadable: {reason}");
            }
            None => panic!("written member reads as map miss"),
        }
        assert!(
            !found.contains_key("absent.txt"),
            "member absence is a map miss, never an Absent variant"
        );
    }
}
