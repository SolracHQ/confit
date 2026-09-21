//! Snapshot
//!
//! Disk snapshots backing drift reads.

use std::path::{Path, PathBuf};

use super::Filesystem;
use crate::document::{ManifestData, ManifestDocument};
use crate::ids::{DocPath, ReadOutcome};

/// One managed file read from a tree destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeMemberRead {
    /// Holds disk bytes plus permission bits for the member.
    Present {
        /// Holds the raw disk bytes.
        bytes: Vec<u8>,
        /// Holds the disk permission bits, None for links.
        mode: Option<u32>,
    },
    /// Holds the raw failure detail from the read.
    Unreadable {
        /// Holds the raw failure detail from the read.
        reason: String,
    },
}

/// Snapshots one tree destination into relative member reads.
///
/// Missing destinations read empty. Symlinks read as raw
/// target text.
///
/// # Arguments
///
/// * `dir` - the expanded destination folder under walking.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Relative member paths mapping to disk reads.
///
pub fn snapshot_tree(
    dir: &Path,
    fs: &dyn Filesystem,
) -> std::collections::BTreeMap<String, TreeMemberRead> {
    let mut out = std::collections::BTreeMap::new();
    walk_tree(dir, dir, fs, &mut out);
    out
}

/// Walks one folder into relative member reads.
fn walk_tree(
    root: &Path,
    dir: &Path,
    fs: &dyn Filesystem,
    out: &mut std::collections::BTreeMap<String, TreeMemberRead>,
) {
    let children = match fs.list_dir(dir) {
        Ok(children) => children,
        Err(_) => return,
    };
    for child in children {
        let rel = match child.strip_prefix(root) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => continue,
        };
        let Some(name) = rel.to_str() else { continue };
        if let Ok(items) = fs.list_dir(&child)
            && !items.is_empty()
        {
            walk_tree(root, &child, fs, out);
            continue;
        }
        if let Some(target) = fs.read_link(&child) {
            out.insert(
                name.to_string(),
                TreeMemberRead::Present {
                    bytes: target.as_os_str().as_encoded_bytes().to_vec(),
                    mode: None,
                },
            );
            continue;
        }
        match fs.read(&child) {
            Ok(bytes) => {
                out.insert(
                    name.to_string(),
                    TreeMemberRead::Present {
                        bytes,
                        mode: fs.file_mode(&child),
                    },
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                out.insert(
                    name.to_string(),
                    TreeMemberRead::Unreadable {
                        reason: error.to_string(),
                    },
                );
            }
        }
    }
}

/// Snapshots one document path through a backend.
///
/// Links snapshot as their raw target text, matching the drift
/// shape for link documents without following the link.
/// Permission bits ride along for mode comparisons.
///
/// # Arguments
///
/// * `path` - the document path with a leading tilde for home targets.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Absent for missing paths, present bytes plus mode for
/// readable files, unreadable holding the failure detail otherwise.
///
pub fn snapshot(path: &DocPath, fs: &dyn Filesystem) -> ReadOutcome {
    let expanded = path.expand();
    if let Some(target) = fs.read_link(&expanded) {
        return ReadOutcome::Present {
            bytes: target.as_os_str().as_encoded_bytes().to_vec(),
            mode: None,
        };
    }
    match fs.read(&expanded) {
        Ok(bytes) => ReadOutcome::Present {
            bytes,
            mode: fs.file_mode(&expanded),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ReadOutcome::Absent,
        Err(error) => ReadOutcome::Unreadable {
            reason: error.to_string(),
        },
    }
}

/// Snapshots one document path following disk symlinks.
///
/// Links resolve against the link parent folder, so relative
/// targets land beside the link. Dangling links read absent,
/// the content they named is gone. Modes follow the bytes,
/// so a followed target compares its own bits.
///
/// # Arguments
///
/// * `path` - the document path with a leading tilde for home targets.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Absent for missing plus dangling paths, present bytes plus
/// mode for readable files, unreadable holding the failure
/// detail otherwise.
///
pub fn snapshot_content(path: &DocPath, fs: &dyn Filesystem) -> ReadOutcome {
    let expanded = path.expand();
    let target = match fs.read_link(&expanded) {
        Some(link) => join_link_target(&expanded, &link),
        None => expanded,
    };
    match fs.read(&target) {
        Ok(bytes) => ReadOutcome::Present {
            bytes,
            mode: fs.file_mode(&target),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ReadOutcome::Absent,
        Err(error) => ReadOutcome::Unreadable {
            reason: error.to_string(),
        },
    }
}

/// Joins a link target against the link parent without touching disk.
///
/// Relative targets resolve beside the link, matching host
/// read behavior on memory backends holding exact paths.
/// Parent segments pop lexically, staying put past the root.
///
/// # Arguments
///
/// * `link` - the expanded link path under resolving.
/// * `target` - the raw target text the link holds.
///
/// # Returns
///
/// The absolute target for absolute text, else the joined
/// plus normalized path beside the link.
///
fn join_link_target(link: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        return target.to_path_buf();
    }
    let mut out = match link.parent() {
        Some(parent) => parent.to_path_buf(),
        None => PathBuf::new(),
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

/// Snapshots one document through its kind-aware reader.
///
/// Link documents read their target text through `snapshot`.
/// Every other kind reads bytes through `snapshot_content`,
/// following disk symlinks to the content behind them.
///
/// # Arguments
///
/// * `document` - the recorded document under snapshotting.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The disk outcome backing drift for the document.
///
pub fn snapshot_document(document: &ManifestDocument, fs: &dyn Filesystem) -> ReadOutcome {
    if matches!(document.data, ManifestData::Link { .. }) {
        snapshot(&document.path, fs)
    } else {
        snapshot_content(&document.path, fs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fs::memory::MemoryFs;

    fn memory_link(fs: &MemoryFs, link: &str, target: &str, bytes: &[u8]) {
        assert!(fs.write(std::path::Path::new(target), bytes).is_ok());
        assert!(
            fs.symlink(std::path::Path::new(link), std::path::Path::new(target))
                .is_ok()
        );
    }

    #[test]
    fn content_reads_behind_disk_links() {
        let fs = MemoryFs::new();
        memory_link(&fs, "link", "behind", b"hi");
        match snapshot_content(&DocPath::new("link"), &fs) {
            ReadOutcome::Present { bytes, .. } => assert_eq!(bytes, b"hi"),
            ReadOutcome::Absent => panic!("link reads absent"),
            ReadOutcome::Unreadable { reason } => panic!("link reads failed: {reason}"),
        }
    }

    #[test]
    fn content_resolves_relative_targets_beside_the_link() {
        let fs = MemoryFs::new();
        assert!(fs.write(std::path::Path::new("behind"), b"hi").is_ok());
        assert!(
            fs.symlink(
                std::path::Path::new("sub/link"),
                std::path::Path::new("../behind")
            )
            .is_ok()
        );
        match snapshot_content(&DocPath::new("sub/link"), &fs) {
            ReadOutcome::Present { bytes, .. } => assert_eq!(bytes, b"hi"),
            ReadOutcome::Absent => panic!("link reads absent"),
            ReadOutcome::Unreadable { reason } => panic!("link reads failed: {reason}"),
        }
    }

    #[test]
    fn content_reads_dangling_links_absent() {
        let fs = MemoryFs::new();
        assert!(
            fs.symlink(
                std::path::Path::new("ghost"),
                std::path::Path::new("nowhere")
            )
            .is_ok()
        );
        match snapshot_content(&DocPath::new("ghost"), &fs) {
            ReadOutcome::Absent => {}
            ReadOutcome::Present { .. } => panic!("dangling reads present"),
            ReadOutcome::Unreadable { reason } => panic!("dangling reads failed: {reason}"),
        }
    }

    #[test]
    fn document_reader_routes_link_docs_to_targets() {
        let fs = MemoryFs::new();
        memory_link(&fs, "link", "behind", b"hi");
        let target = ManifestDocument::new(
            DocPath::new("link"),
            ManifestData::Link {
                target: "behind".into(),
            },
        );
        match snapshot_document(&target, &fs) {
            ReadOutcome::Present { bytes, .. } => assert_eq!(bytes, b"behind"),
            ReadOutcome::Absent => panic!("target reads absent"),
            ReadOutcome::Unreadable { reason } => panic!("target reads failed: {reason}"),
        }
        let text = ManifestDocument::new(
            DocPath::new("link"),
            ManifestData::Text {
                content: "hi".into(),
                mode: None,
                unmanaged: false,
            },
        );
        match snapshot_document(&text, &fs) {
            ReadOutcome::Present { bytes, .. } => assert_eq!(bytes, b"hi"),
            ReadOutcome::Absent => panic!("text reads absent"),
            ReadOutcome::Unreadable { reason } => panic!("text reads failed: {reason}"),
        }
    }
}
