//! Disk input with writes and removals.

use std::collections::BTreeMap;
use std::path::Path;

use confit_driver as driver;
#[cfg(any(unix, test))]
use confit_model::document::Data;
use confit_model::document::{Payload, TreeMember};
use confit_model::routes::Route;
use confit_store::Stores;
use confit_store::blob::error::BlobError;
use confit_store::faults::AccessFault;

use crate::error::{Result, RuntimeError};
use crate::manifest::Document;

/// Disk state.
pub enum Disk {
    /// Missing destination.
    Absent,
    /// Unreadable destination.
    Unreadable {
        /// The fault cause for the failed read.
        fault: AccessFault,
    },
    /// Present destination.
    Present {
        /// Reader for destination bytes.
        reader: Box<dyn std::io::Read>,
        /// Unix permission bits.
        mode: Option<u32>,
    },
}

/// Disk tree member state.
pub enum DiskMember {
    /// Unreadable member.
    Unreadable {
        /// The fault cause for the failed read.
        fault: AccessFault,
    },
    /// Present member.
    Present {
        /// Reader for member bytes.
        reader: Box<dyn std::io::Read>,
        /// Unix permission bits.
        mode: Option<u32>,
    },
}

/// Open one document destination through its kind-aware reader.
pub fn disk_doc(document: &Document) -> Disk {
    let expanded = match document.destination.expand() {
        Ok(path) => path,
        Err(error) => {
            return Disk::Unreadable {
                fault: AccessFault::Unknown {
                    message: error.to_string(),
                },
            };
        }
    };
    let link_doc = matches!(document.data, Data::Link(_));
    let target = match std::fs::read_link(&expanded) {
        Ok(link) => join_link_target(&expanded, &link),
        Err(_) => expanded.clone(),
    };
    if link_doc {
        return match driver::fs::read_link_bytes(&expanded) {
            Ok(bytes) => Disk::Present {
                reader: Box::new(std::io::Cursor::new(bytes)),
                mode: None,
            },
            Err(_) if expanded.is_dir() => dir_unreadable(&expanded),
            Err(_) => match std::fs::File::open(&expanded) {
                Ok(file) => Disk::Present {
                    reader: Box::new(file),
                    mode: file_mode(&expanded),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Disk::Absent,
                Err(error) => Disk::Unreadable {
                    fault: disk_fault(error),
                },
            },
        };
    }
    if target.is_dir() {
        return dir_unreadable(&target);
    }
    match std::fs::File::open(&target) {
        Ok(file) => Disk::Present {
            reader: Box::new(file),
            mode: file_mode(&target),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Disk::Absent,
        Err(error) => Disk::Unreadable {
            fault: disk_fault(error),
        },
    }
}

/// Open one directory destination as unreadable.
fn dir_unreadable(target: &Path) -> Disk {
    let error = std::io::Error::new(
        std::io::ErrorKind::IsADirectory,
        format!("is a directory '{}'", target.display()),
    );
    Disk::Unreadable {
        fault: disk_fault(error),
    }
}

/// Open one tree destination into relative member readers.
pub fn disk_tree(document: &Document) -> BTreeMap<String, DiskMember> {
    let Ok(dir) = document.destination.expand() else {
        return BTreeMap::new();
    };
    let mut out = BTreeMap::new();
    walk_tree(&dir, &dir, &mut out);
    out
}

/// Open a fresh reader for one tree member path.
pub fn open_member(destination: &Route, relative: &str) -> Option<Box<dyn std::io::Read>> {
    let dest = destination.expand().ok()?;
    let path = dest.join(relative);
    if let Ok(bytes) = driver::fs::read_link_bytes(&path) {
        return Some(Box::new(std::io::Cursor::new(bytes)) as Box<dyn std::io::Read>);
    }
    std::fs::File::open(&path)
        .ok()
        .map(|file| Box::new(file) as Box<dyn std::io::Read>)
}

/// Write one tree destination member by member.
///
/// # Errors
///
/// - [`RuntimeError::MissingBlob`] for dangling member
///   hashes with the destination and the member.
/// - [`RuntimeError::Write`] for unwritable members
///   and mode failures.
/// - [`RuntimeError::WriteUnknown`] for unmapped
///   member failures.
pub fn write_tree(
    dest: &Path,
    members: &BTreeMap<String, TreeMember>,
    stores: &Stores,
) -> Result<usize> {
    for (relative, member) in members {
        let path = dest.join(relative);
        copy_blob(dest, &path, member.blob(), stores)?;
        driver::fs::set_mode(&path, Some(member.mode()))
            .map_err(|error| RuntimeError::from_write_io(&path, error))?;
    }
    Ok(1)
}

/// Remove one backend path and report whether anything remains.
pub fn remove(path: &Path) -> std::io::Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path)?;
    Ok(true)
}

/// Walk one folder into relative member readers.
///
/// - Missing folders are empty.
fn walk_tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, DiskMember>) {
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
        if let Ok(bytes) = driver::fs::read_link_bytes(&child) {
            out.insert(
                name.to_string(),
                DiskMember::Present {
                    reader: Box::new(std::io::Cursor::new(bytes)),
                    mode: None,
                },
            );
            continue;
        }
        match std::fs::File::open(&child) {
            Ok(file) => {
                out.insert(
                    name.to_string(),
                    DiskMember::Present {
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
                    DiskMember::Unreadable {
                        fault: disk_fault(error),
                    },
                );
            }
        }
    }
}

/// Join a link target against the link parent.
///
/// - Relative targets resolve beside the link.
/// - Parent segments pop lexically. They stay put past the root.
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

/// Read unix permission bits for one path.
fn file_mode(path: &Path) -> Option<u32> {
    if path.is_symlink() {
        return None;
    }
    driver::fs::mode(path).ok()
}

/// Stream one blob to its destination through the blob pool.
///
/// # Errors
///
/// - [`RuntimeError::MissingBlob`] for dangling hashes.
/// - [`RuntimeError::Write`] for unwritable destinations.
/// - [`RuntimeError::WriteUnknown`] for unmapped
///   destination failures.
pub(crate) fn copy_blob(
    dest: &Path,
    target: &Path,
    payload: &Payload,
    stores: &Stores,
) -> Result<()> {
    use std::io::Write as _;

    let resolved = payload
        .resolve(stores)
        .map_err(|error| map_pool(error, dest, target))?;
    let handle = stores
        .blobs()
        .resolve(&resolved)
        .map_err(|error| map_pool(error, dest, target))?;
    let mut reader = stores
        .blobs()
        .open(&handle)
        .map_err(|error| map_pool(error, dest, target))?;
    driver::fs::create_parent(target)
        .map_err(|error| RuntimeError::from_write_io(target, error))?;
    let mut out = std::fs::File::create(target)
        .map_err(|error| RuntimeError::from_write_io(target, error))?;
    std::io::copy(&mut reader, &mut out)
        .map_err(|error| RuntimeError::from_write_io(target, error))?;
    out.flush()
        .map_err(|error| RuntimeError::from_write_io(target, error))?;
    Ok(())
}

/// Map one pool failure at the destination into runtime language.
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
        BlobError::Compress => {
            log::error!("cannot compress blob for '{}'", target.display());
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message: "cannot compress blob".to_owned(),
            }
        }
        BlobError::Corrupt { sha } => {
            log::error!("corrupt blob '{}' for '{}'", sha.hex(), target.display());
            RuntimeError::WriteUnknown {
                path: target.to_path_buf(),
                message: format!("corrupt blob '{}'", sha.hex()),
            }
        }
    }
}

/// Interpret one disk read failure into its fault cause.
fn disk_fault(error: std::io::Error) -> AccessFault {
    let kind = error.kind();
    let message = error.to_string();
    log::error!("disk read failed: {message}");
    match AccessFault::interpret(kind) {
        Some(fault) => fault,
        None => AccessFault::Unknown { message },
    }
}

#[cfg(test)]
mod disk_tests {
    use super::*;
    use confit_model::document::TextDocument;
    use confit_model::routes::{Route, RouteBase};

    fn literal(path: &Path) -> Route {
        Route::new(RouteBase::Literal, path).unwrap()
    }

    fn text_doc(path: &Path) -> Document {
        Document::new(
            literal(path),
            Data::Text(TextDocument {
                content: "disk bytes".into(),
                mode: 0o644,
                unmanaged: false,
            }),
        )
    }

    #[test]
    fn absent_doc_reads_absent() {
        let dir = tempfile::tempdir().unwrap();
        match disk_doc(&text_doc(&dir.path().join("absent.txt"))) {
            Disk::Absent => {}
            Disk::Present { .. } => panic!("never-written path is present"),
            Disk::Unreadable { fault } => {
                panic!("never-written path is unreadable: {fault}")
            }
        }
    }

    #[test]
    fn present_doc_streams_bytes_plus_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("present.txt");
        driver::fs::create_parent(&path).unwrap();
        std::fs::write(&path, b"disk bytes").unwrap();
        driver::fs::set_mode(&path, Some(0o755)).unwrap();
        match disk_doc(&text_doc(&path)) {
            Disk::Present { mut reader, mode } => {
                let mut seen = Vec::new();
                reader.read_to_end(&mut seen).unwrap();
                assert_eq!(seen, b"disk bytes".to_vec());
                assert_eq!(mode, Some(0o755), "disk mode matches the reader");
            }
            Disk::Absent => panic!("written path is absent"),
            Disk::Unreadable { fault } => {
                panic!("written path is unreadable: {fault}")
            }
        }
    }

    #[test]
    fn unreadable_doc_names_the_reason() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("blocked.txt");
        std::fs::create_dir_all(&path).unwrap();
        match disk_doc(&text_doc(&path)) {
            Disk::Unreadable { fault } => assert!(
                !fault.cause().is_empty(),
                "unreadable carries the fault cause"
            ),
            Disk::Absent => panic!("directory path is absent"),
            Disk::Present { .. } => panic!("directory path is present"),
        }
    }
}
