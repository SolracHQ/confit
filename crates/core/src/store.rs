//! Store
//!
//! Manifests plus blob pool plus document writes plus history rotation.

pub mod blobs;
pub mod bundle;
pub mod manifest;
pub mod slots;

use std::collections::BTreeMap;
use std::path::Path;

use crate::document::{ManifestData, ManifestDocument, ManifestMember};
use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::ids::DocPath;

/// Writes every document to its expanded path.
///
/// A pre-existing symlink under a plain document unlinks
/// first, leaving its target alone, then the fresh regular
/// file lands in its place.
///
/// # Arguments
///
/// * `documents` - the desired documents under writing.
/// * `blobs` - the raw blob bytes under content hashes.
/// * `fs` - the backend under writing.
/// * `on_written` - the per-document callback, holding `None` for silence.
///
/// # Returns
///
/// Unit once every document lands.
///
/// # Errors
///
/// Render plus io failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::store::write_documents;
/// use std::collections::BTreeMap;
///
/// let fs = MemoryFs::new();
/// let documents = vec![ManifestDocument::new(
///     DocPath::new("note"),
///     ManifestData::Text { content: "hi".into(), mode: None },
/// )];
/// assert!(write_documents(&documents, &BTreeMap::new(), &fs, None).is_ok());
/// assert!(fs.exists(std::path::Path::new("note")));
/// ```
pub fn write_documents(
    documents: &[ManifestDocument],
    blobs: &BTreeMap<String, Vec<u8>>,
    fs: &dyn Filesystem,
    on_written: Option<&dyn Fn(&DocPath)>,
) -> Result<()> {
    for document in documents {
        let expanded = document.path.expand();
        let outcome = match &document.data {
            ManifestData::Link { target } => fs.symlink(&expanded, Path::new(target)),
            ManifestData::Tree { members } => write_tree_members(&expanded, members, blobs, fs),
            _ => {
                if fs.read_link(&expanded).is_some()
                    && let Err(error) = fs.remove(&expanded)
                {
                    return Err(Error::Plan(format!(
                        "cannot remove link '{}': {error}",
                        expanded.display()
                    )));
                }
                let bytes = document.bytes(blobs)?;
                fs.write(&expanded, &bytes)
            }
        };
        if let Err(error) = outcome {
            return Err(Error::Plan(format!(
                "cannot write '{}': {error}",
                expanded.display()
            )));
        }
        if let Some(mode) = document.mode()
            && let Err(error) = fs.set_mode(&expanded, mode)
        {
            return Err(Error::Plan(format!(
                "cannot set mode '{}': {error}",
                expanded.display()
            )));
        }
        if let Some(notify) = on_written {
            notify(&document.path);
        }
    }
    Ok(())
}

/// Writes one tree destination member by member.
///
/// Only the members land, never the destination folder
/// itself. Parent folders create as needed, modes land
/// per member from the manifest. Member bytes read from
/// the blob map under their references.
///
/// # Arguments
///
/// * `dest` - the expanded destination folder.
/// * `members` - the desired members under writing.
/// * `blobs` - the raw blob bytes under content hashes.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once every member lands.
///
/// # Errors
///
/// Missing blobs plus write plus mode failures surface as
/// io errors carrying the member path.
fn write_tree_members(
    dest: &std::path::Path,
    members: &[ManifestMember],
    blobs: &BTreeMap<String, Vec<u8>>,
    fs: &dyn Filesystem,
) -> std::io::Result<()> {
    for member in members {
        let path = dest.join(&member.relative);
        let content = blobs.get(&member.blob).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("missing blob '{}' for '{}'", member.blob, path.display()),
            )
        })?;
        fs.write(&path, content).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("cannot write '{}': {error}", path.display()),
            )
        })?;
        fs.set_mode(&path, member.mode).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("cannot set mode '{}': {error}", path.display()),
            )
        })?;
    }
    Ok(())
}

/// Removes recorded paths absent from desired documents.
///
/// Only state-recorded paths delete, never anything else.
/// Tree destinations never delete as paths, members
/// reconcile through `remove_tree_members` instead.
/// Already-absent paths stay quiet, matching desired state.
///
/// # Arguments
///
/// * `recorded` - the last recorded documents.
/// * `desired` - the desired documents keeping their paths.
/// * `fs` - the backend under removal.
///
/// # Returns
///
/// The removed path count.
///
/// # Errors
///
/// Removal failures surface as io errors.
///
pub fn remove_orphans(
    recorded: &[ManifestDocument],
    desired: &[ManifestDocument],
    fs: &dyn Filesystem,
) -> Result<usize> {
    let mut removed = 0;
    for old in recorded {
        if old.data.tree_members().is_some() {
            continue;
        }
        let kept = desired.iter().any(|document| document.path == old.path);
        if kept {
            continue;
        }
        let expanded = old.path.expand();
        if !fs.exists(&expanded) {
            continue;
        }
        fs.remove(&expanded).map_err(Error::from)?;
        removed += 1;
    }
    Ok(removed)
}

/// Removes dropped tree members between recorded and desired plans.
///
/// The removal set holds recorded manifest members absent
/// from the desired manifest at the same destination, so
/// files the tree dropped delete while hand-placed files
/// stay untouched. Whole dropped trees remove every
/// recorded member. Destinations never delete.
///
/// # Arguments
///
/// * `recorded` - the last recorded documents.
/// * `desired` - the desired documents holding new manifests.
/// * `fs` - the backend under removal.
///
/// # Returns
///
/// The removed member count.
///
/// # Errors
///
/// Removal failures surface as io errors.
///
pub fn remove_tree_members(
    recorded: &[ManifestDocument],
    desired: &[ManifestDocument],
    fs: &dyn Filesystem,
) -> Result<usize> {
    let mut removed = 0;
    for old in recorded {
        let Some(old_members) = old.data.tree_members() else {
            continue;
        };
        let new_rels: std::collections::BTreeSet<&str> = desired
            .iter()
            .filter(|document| document.path == old.path)
            .filter_map(|document| document.data.tree_members())
            .flat_map(|members| members.iter().map(|member| member.relative.as_str()))
            .collect();
        let dest = old.path.expand();
        for member in old_members {
            if new_rels.contains(member.relative.as_str()) {
                continue;
            }
            let path = dest.join(&member.relative);
            if !fs.exists(&path) {
                continue;
            }
            fs.remove(&path).map_err(Error::from)?;
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree_recorded() -> Vec<ManifestDocument> {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::ids::DocPath;

        vec![ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree {
                members: vec![
                    ManifestMember {
                        relative: "kept.ttf".into(),
                        blob: crate::plan::sha256_hex(&[1]),
                        mode: 0o644,
                    },
                    ManifestMember {
                        relative: "gone.ttf".into(),
                        blob: crate::plan::sha256_hex(&[2]),
                        mode: 0o644,
                    },
                ],
            },
        )]
    }

    fn tree_blobs() -> BTreeMap<String, Vec<u8>> {
        BTreeMap::from([
            (crate::plan::sha256_hex(&[1]), vec![1]),
            (crate::plan::sha256_hex(&[2]), vec![2]),
        ])
    }

    #[test]
    fn write_tree_members_land_with_modes() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        assert!(write_documents(&tree_recorded(), &tree_blobs(), &fs, None).is_ok());
        let dest = std::path::Path::new("fonts");
        match fs.read(&dest.join("kept.ttf")) {
            Ok(bytes) => assert_eq!(bytes, vec![1]),
            Err(error) => panic!("member reads: {error}"),
        }
        assert_eq!(fs.file_mode(&dest.join("kept.ttf")), Some(0o644));
        assert!(fs.exists(&dest.join("gone.ttf")));
    }

    #[test]
    fn plain_writes_replace_disk_links_leaving_targets() {
        use crate::document::{ManifestData, ManifestDocument};
        use crate::fs::{Filesystem, MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        assert!(fs.write(std::path::Path::new("behind"), b"old").is_ok());
        assert!(
            fs.symlink(std::path::Path::new("link"), std::path::Path::new("behind"))
                .is_ok()
        );
        let documents = vec![ManifestDocument::new(
            DocPath::new("link"),
            ManifestData::Text {
                content: "new".into(),
                mode: None,
            },
        )];
        assert!(write_documents(&documents, &std::collections::BTreeMap::new(), &fs, None).is_ok());
        assert!(fs.read_link(std::path::Path::new("link")).is_none());
        match fs.read(std::path::Path::new("behind")) {
            Ok(bytes) => assert_eq!(bytes, b"old"),
            Err(error) => panic!("target reads: {error}"),
        }
        match fs.read(std::path::Path::new("link")) {
            Ok(bytes) => assert_eq!(bytes, b"new"),
            Err(error) => panic!("fresh reads: {error}"),
        }
    }

    #[test]
    fn remove_tree_members_drops_only_dropped() {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::fs::{Filesystem, MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        assert!(write_documents(&tree_recorded(), &tree_blobs(), &fs, None).is_ok());
        let dest = std::path::Path::new("fonts");
        match fs.write(&dest.join("hand.ttf"), b"mine") {
            Ok(()) => {}
            Err(error) => panic!("hand writes: {error}"),
        }
        let desired = vec![ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree {
                members: vec![ManifestMember {
                    relative: "kept.ttf".into(),
                    blob: crate::plan::sha256_hex(&[1]),
                    mode: 0o644,
                }],
            },
        )];
        match remove_tree_members(&tree_recorded(), &desired, &fs) {
            Ok(removed) => assert_eq!(removed, 1),
            Err(error) => panic!("members remove: {error}"),
        }
        assert!(fs.exists(&dest.join("kept.ttf")));
        assert!(!fs.exists(&dest.join("gone.ttf")));
        assert!(fs.exists(&dest.join("hand.ttf")));
    }

    #[test]
    fn remove_orphans_skips_tree_destinations() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        assert!(write_documents(&tree_recorded(), &tree_blobs(), &fs, None).is_ok());
        match remove_orphans(&tree_recorded(), &[], &fs) {
            Ok(removed) => assert_eq!(removed, 0),
            Err(error) => panic!("orphans remove: {error}"),
        }
        let dest = std::path::Path::new("fonts");
        assert!(fs.exists(&dest.join("kept.ttf")));
    }
}
