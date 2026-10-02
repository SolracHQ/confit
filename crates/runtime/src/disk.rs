//! Disk backend behind live reads, writes, and removals.

use std::collections::BTreeMap;
use std::path::Path;

use confit_driver as driver;
use confit_model::document::{BlobRef, Data, Document, ManifestMember};
use confit_model::routes::Route;
use confit_store::blob::BlobStore;
use confit_store::blob::error::BlobError;
use confit_store::faults::AccessFault;

use crate::Applier;
use crate::error::{Result, RuntimeError};

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
    /// Failing read carrying its fault cause.
    Unreadable {
        /// Holds the fault cause behind the failed read.
        fault: AccessFault,
    },
    /// Content reader and permission bits for the destination.
    Present {
        /// Streams destination bytes.
        reader: Box<dyn std::io::Read>,
        /// Holds unix permission bits, None for links.
        mode: Option<u32>,
    },
}

/// Live disk state behind one tree member.
///
/// Present and unreadable members enter the map keyed by
/// relative path. Missing members stay out, so absence
/// reads as a map miss.
pub enum LiveMember {
    /// Failing read carrying its fault cause.
    Unreadable {
        /// Holds the fault cause behind the failed read.
        fault: AccessFault,
    },
    /// Content reader and permission bits for the member.
    Present {
        /// Streams member bytes.
        reader: Box<dyn std::io::Read>,
        /// Holds unix permission bits, None for links.
        mode: Option<u32>,
    },
}

impl HostDisk {
    /// Reads one document destination through its kind-aware reader.
    pub fn live_doc(&self, document: &Document) -> Live {
        let expanded = document.destination.expand();
        let is_link = matches!(document.data, Data::Link { .. });
        live_doc_host(&expanded, is_link)
    }

    /// Reads one tree destination into relative member readers.
    pub fn live_tree(&self, document: &Document) -> BTreeMap<String, LiveMember> {
        let dir = document.destination.expand();
        live_tree_host(&dir)
    }

    /// Opens a fresh streaming reader for one tree member path.
    pub fn open_member(
        &self,
        destination: &Route,
        relative: &str,
    ) -> Option<Box<dyn std::io::Read>> {
        let dest = destination.expand();
        let path = dest.join(relative);
        if let Ok(target) = driver::fs::read_link(&path) {
            return Some(Box::new(std::io::Cursor::new(
                target.as_os_str().as_encoded_bytes().to_vec(),
            )) as Box<dyn std::io::Read>);
        }
        driver::fs::open(&path)
            .ok()
            .map(|file| file as Box<dyn std::io::Read>)
    }

    /// Reports whether one backend path reads present.
    pub fn exists(&self, path: &Path) -> bool {
        driver::fs::exists(path)
    }

    /// Seeds one file behind a resolved path for memory runs.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        let _ = self.write_bytes(path, bytes);
    }

    /// Writes bytes to one path, creating parents as needed.
    pub fn write_bytes(&self, path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        ensure_parent(path)?;
        driver::fs::write(path, bytes)
    }

    /// Streams one blob to its destination through the blob pool.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::MissingBlob`] for dangling hashes.
    /// - [`RuntimeError::Write`] for unwritable destinations.
    /// - [`RuntimeError::WriteUnknown`] for unmapped
    ///   destination failures.
    pub fn write_blob(&self, dest: &Path, blob: &BlobRef, blobs: &BlobStore) -> Result<()> {
        copy_blob(dest, dest, blob, blobs)
    }

    /// Writes one tree destination member by member.
    ///
    /// # Errors
    ///
    /// - [`RuntimeError::MissingBlob`] for dangling member
    ///   hashes naming the destination with the member.
    /// - [`RuntimeError::Write`] for unwritable members
    ///   and mode failures.
    /// - [`RuntimeError::WriteUnknown`] for unmapped
    ///   member failures.
    pub fn write_tree(
        &self,
        dest: &Path,
        members: &[ManifestMember],
        blobs: &BlobStore,
    ) -> Result<usize> {
        for member in members {
            let path = dest.join(&member.relative);
            copy_blob(dest, &path, &member.blob, blobs)?;
            driver::fs::set_mode(&path, member.mode)
                .map_err(|error| RuntimeError::from_write_io(&path, error))?;
        }
        Ok(1)
    }

    /// Creates one symlink, replacing present files.
    pub fn write_link(&self, link: &Path, target: &Path) -> std::io::Result<()> {
        driver::fs::write_link(link, target)
    }

    /// Clears one symlink standing where a file lands.
    pub fn clear_link(&self, path: &Path) -> std::io::Result<()> {
        if driver::fs::read_link(path).is_ok() {
            driver::fs::remove_file(path)?;
        }
        Ok(())
    }

    /// Sets unix permission bits on one path.
    pub fn set_mode(&self, path: &Path, mode: u32) -> std::io::Result<()> {
        driver::fs::set_mode(path, mode)
    }

    /// Removes one backend path, reporting whether anything left.
    pub fn remove(&self, path: &Path) -> std::io::Result<bool> {
        if !driver::fs::exists(path) {
            return Ok(false);
        }
        driver::fs::remove_file(path)?;
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
    let target = match driver::fs::read_link(expanded) {
        Ok(link) => join_link_target(expanded, &link),
        Err(_) => expanded.to_path_buf(),
    };
    if is_link {
        return match driver::fs::read_link(expanded) {
            Ok(link) => Live::Present {
                reader: Box::new(std::io::Cursor::new(
                    link.as_os_str().as_encoded_bytes().to_vec(),
                )),
                mode: None,
            },
            Err(_) => match driver::fs::open(expanded) {
                Ok(file) => Live::Present {
                    reader: Box::new(file),
                    mode: file_mode(expanded),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Live::Absent,
                Err(error) => Live::Unreadable {
                    fault: live_fault(error),
                },
            },
        };
    }
    match driver::fs::open(&target) {
        Ok(file) => Live::Present {
            reader: Box::new(file),
            mode: file_mode(&target),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Live::Absent,
        Err(error) => Live::Unreadable {
            fault: live_fault(error),
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
    let children = match driver::fs::read_dir(dir) {
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
        if let Ok(items) = driver::fs::read_dir(&child)
            && !items.is_empty()
        {
            walk_tree(root, &child, out);
            continue;
        }
        if let Ok(target) = driver::fs::read_link(&child) {
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
        match driver::fs::open(&child) {
            Ok(file) => {
                out.insert(
                    name.to_string(),
                    LiveMember::Present {
                        reader: Box::new(file),
                        mode: file_mode(&child),
                    },
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                continue;
            }
            Err(error) => {
                out.insert(
                    name.to_string(),
                    LiveMember::Unreadable {
                        fault: live_fault(error),
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
    if driver::fs::read_link(path).is_ok() {
        return None;
    }
    driver::fs::mode(path).ok()
}

/// Drains one streaming reader into bytes.
pub(crate) fn drain(reader: &mut dyn std::io::Read) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.read_to_end(&mut out)?;
    Ok(out)
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
        driver::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Streams one blob to its destination through the blob pool.
///
/// # Errors
///
/// - [`RuntimeError::MissingBlob`] for dangling hashes.
/// - [`RuntimeError::Write`] for unwritable destinations.
/// - [`RuntimeError::WriteUnknown`] for unmapped
///   destination failures.
fn copy_blob(dest: &Path, target: &Path, blob: &BlobRef, blobs: &BlobStore) -> Result<()> {
    use std::io::Write as _;

    let handle = blobs
        .resolve(blob)
        .map_err(|error| map_pool(error, dest, target))?;
    let mut reader = blobs
        .open(&handle)
        .map_err(|error| map_pool(error, dest, target))?;
    ensure_parent(target).map_err(|error| RuntimeError::from_write_io(target, error))?;
    let mut out =
        driver::fs::create(target).map_err(|error| RuntimeError::from_write_io(target, error))?;
    std::io::copy(&mut reader, &mut out)
        .map_err(|error| RuntimeError::from_write_io(target, error))?;
    out.flush()
        .map_err(|error| RuntimeError::from_write_io(target, error))?;
    Ok(())
}

/// Maps one pool failure at the destination into runtime language.
fn map_pool(error: BlobError, dest: &Path, target: &Path) -> RuntimeError {
    match error {
        BlobError::Read {
            sha,
            fault: AccessFault::Missing,
        } => {
            log::error!("missing blob '{}' for '{}'", sha.hex(), target.display());
            RuntimeError::MissingBlob {
                dest: dest.to_path_buf(),
                sha,
                member: target.to_path_buf(),
            }
        }
        BlobError::Read {
            fault: AccessFault::Unknown { message },
            ..
        } => {
            log::error!("pool read failed: {message}");
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message,
            }
        }
        BlobError::Read { fault, .. } => RuntimeError::Write {
            path: target.to_path_buf(),
            fault,
        },
        BlobError::Write { fault, .. } => RuntimeError::Write {
            path: target.to_path_buf(),
            fault,
        },
        BlobError::WriteUnknown { message, .. } => {
            log::error!("pool write failed: {message}");
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message,
            }
        }
        BlobError::Corrupt { sha } => {
            log::error!("corrupt blob '{}'", sha.hex());
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message: format!("blob '{}' fails verification", sha.hex()),
            }
        }
        BlobError::Compress => {
            log::error!("cannot compress blob for '{}'", target.display());
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message: "cannot compress blob".to_owned(),
            }
        }
    }
}

/// Interprets one live read failure into its fault cause.
fn live_fault(error: std::io::Error) -> AccessFault {
    let kind = error.kind();
    let message = error.to_string();
    log::error!("live read failed: {message}");
    match AccessFault::interpret(kind) {
        Some(fault) => fault,
        None => AccessFault::Unknown { message },
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use confit_driver::fs::TestGuard;
    use confit_model::routes::{Route, RouteBase};

    fn literal(path: &Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &Path) -> Document {
        Document::new(
            literal(path),
            Data::Text {
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
            Live::Unreadable { fault } => {
                panic!("never-written path reads unreadable: {fault}")
            }
        }
    }

    #[test]
    fn present_doc_streams_bytes_plus_mode() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("present.txt");
        let disk = HostDisk;
        disk.write_bytes(&path, b"live bytes").unwrap();
        driver::fs::set_mode(&path, 0o755).unwrap();
        match disk.live_doc(&text_doc(&path)) {
            Live::Present { mut reader, mode } => {
                assert_eq!(drain(&mut reader).unwrap(), b"live bytes".to_vec());
                assert_eq!(mode, Some(0o755), "live mode rides beside the reader");
            }
            Live::Absent => panic!("written path reads absent"),
            Live::Unreadable { fault } => {
                panic!("written path reads unreadable: {fault}")
            }
        }
    }

    #[test]
    fn unreadable_doc_names_the_reason() {
        let _guard = TestGuard::install();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked.txt");
        driver::fs::create_dir_all(&path).unwrap();
        let disk = HostDisk;
        match disk.live_doc(&text_doc(&path)) {
            Live::Unreadable { fault } => assert!(
                !fault.cause().is_empty(),
                "unreadable carries the fault cause"
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
        let document = Document::new(
            literal(&root),
            Data::Tree {
                members: Vec::new(),
            },
        );
        let mut found = disk.live_tree(&document);
        match found.get_mut("present.txt") {
            Some(LiveMember::Present { reader, .. }) => {
                assert_eq!(drain(reader).unwrap(), b"member bytes".to_vec());
            }
            Some(LiveMember::Unreadable { fault }) => {
                panic!("written member reads unreadable: {fault}");
            }
            None => panic!("written member reads as map miss"),
        }
        assert!(
            !found.contains_key("absent.txt"),
            "member absence is a map miss, never an Absent variant"
        );
    }
}
