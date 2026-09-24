//! Disk snapshots behind drift comparison.

use std::collections::BTreeMap;
use std::path::Path;

use confit_core::document::ManifestDocument;

use crate::Applier;

/// Streaming disk state behind one document destination.
///
/// Readers stream content without loading whole files.
/// Modes ride beside readers for permission comparison.
pub enum Snapshot {
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

/// Streaming disk state behind one tree member.
pub enum TreeMemberSnapshot {
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

impl Applier {
    /// Snapshots one document destination through its kind-aware reader.
    pub fn snapshot_doc(&self, document: &ManifestDocument) -> Snapshot {
        self.disk.snapshot_doc(document)
    }

    /// Snapshots one tree destination into relative member readers.
    pub fn snapshot_tree(
        &self,
        document: &ManifestDocument,
    ) -> BTreeMap<String, TreeMemberSnapshot> {
        self.disk.snapshot_tree(document)
    }
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

/// Snapshots one host document destination through its kind-aware reader.
pub(crate) fn snapshot_doc_host(expanded: &Path, is_link: bool) -> Snapshot {
    let target = match std::fs::read_link(expanded) {
        Ok(link) => join_link_target(expanded, &link),
        Err(_) => expanded.to_path_buf(),
    };
    if is_link {
        return match std::fs::read_link(expanded) {
            Ok(link) => Snapshot::Present {
                reader: Box::new(std::io::Cursor::new(
                    link.as_os_str().as_encoded_bytes().to_vec(),
                )),
                mode: None,
            },
            Err(_) => match std::fs::File::open(expanded) {
                Ok(file) => Snapshot::Present {
                    reader: Box::new(file),
                    mode: file_mode(expanded),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Snapshot::Absent,
                Err(error) => Snapshot::Unreadable {
                    reason: error.to_string(),
                },
            },
        };
    }
    match std::fs::File::open(&target) {
        Ok(file) => Snapshot::Present {
            reader: Box::new(file),
            mode: file_mode(&target),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Snapshot::Absent,
        Err(error) => Snapshot::Unreadable {
            reason: error.to_string(),
        },
    }
}

/// Snapshots one host tree destination into relative member readers.
pub(crate) fn snapshot_tree_host(dir: &Path) -> BTreeMap<String, TreeMemberSnapshot> {
    let mut out = BTreeMap::new();
    walk_tree(dir, dir, &mut out);
    out
}

/// Walks one folder into relative member readers.
///
/// Missing folders read empty.
fn walk_tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, TreeMemberSnapshot>) {
    let children = match std::fs::read_dir(dir) {
        Ok(children) => children,
        Err(_) => return,
    };
    for child in children {
        let child = match child {
            Ok(child) => child.path(),
            Err(_) => continue,
        };
        let rel = match child.strip_prefix(root) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => continue,
        };
        let Some(name) = rel.to_str() else {
            continue;
        };
        if let Ok(mut items) = std::fs::read_dir(&child)
            && items.next().is_some()
        {
            walk_tree(root, &child, out);
            continue;
        }
        if let Ok(target) = std::fs::read_link(&child) {
            out.insert(
                name.to_string(),
                TreeMemberSnapshot::Present {
                    reader: Box::new(std::io::Cursor::new(
                        target.as_os_str().as_encoded_bytes().to_vec(),
                    )),
                    mode: None,
                },
            );
            continue;
        }
        match std::fs::File::open(&child) {
            Ok(file) => {
                out.insert(
                    name.to_string(),
                    TreeMemberSnapshot::Present {
                        reader: Box::new(file),
                        mode: file_mode(&child),
                    },
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                out.insert(
                    name.to_string(),
                    TreeMemberSnapshot::Unreadable {
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
pub(crate) fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() {
        return None;
    }
    Some(metadata.permissions().mode() & 0o777)
}
