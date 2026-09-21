//! Store
//!
//! Manifests, blob pool, document writes, and history rotation.

pub mod blobs;
pub mod bundle;
pub mod manifest;
pub mod slots;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::document::{ManifestData, ManifestDocument, ManifestMember};
use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::ids::DocPath;
use crate::store::blobs::blob_source;
use crate::store::blobs::{BlobRef, BlobSource};

/// Writes every document to its expanded path.
///
/// A pre-existing symlink under a plain document unlinks
/// first, leaving its target alone, then the fresh regular
/// file lands in its place. Present unmanaged documents
/// stay untouched while their declaration matches the
/// recorded manifest, missing, and rewritten ones write
/// normally. Opaque and tree bytes resolve pool-first, then
/// stream to disk without holding whole files.
///
/// # Arguments
///
/// * `documents` - the desired documents under writing.
/// * `blobs` - the blob refs under content hashes.
/// * `fs` - the backend under writing.
/// * `on_written` - the per-document callback, holding `None` for silence.
/// * `rewritten` - the declaration-changed paths under rewriting.
///
/// # Returns
///
/// The actual write count, skipping untouched unmanaged documents.
///
/// # Errors
///
/// Render and io failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{ManifestData, ManifestDocument};
/// use confit_core::fs::{Filesystem, memory::MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::store::write_documents;
/// use std::collections::{BTreeMap, BTreeSet};
///
/// let fs = MemoryFs::new();
/// let documents = vec![ManifestDocument::new(
///     DocPath::new("note"),
///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false },
/// )];
/// let written = write_documents(&documents, &BTreeMap::new(), &fs, None, &BTreeSet::new());
/// assert!(matches!(written, Ok(1)));
/// assert!(fs.exists(std::path::Path::new("note")));
/// ```
pub fn write_documents(
    documents: &[ManifestDocument],
    blobs: &BTreeMap<String, BlobRef>,
    fs: &dyn Filesystem,
    on_written: Option<&dyn Fn(&DocPath)>,
    rewritten: &BTreeSet<DocPath>,
) -> Result<usize> {
    let mut written = 0;
    for document in documents {
        let expanded = document.path.expand();
        if document.data.unmanaged() && fs.exists(&expanded) && !rewritten.contains(&document.path)
        {
            continue;
        }
        let outcome = match &document.data {
            ManifestData::Link { target } => fs
                .symlink(&expanded, Path::new(target))
                .map_err(Error::from),
            ManifestData::Tree { members } => write_tree_members(&expanded, members, blobs, fs),
            ManifestData::Opaque { blob, .. } => {
                if fs.read_link(&expanded).is_some()
                    && let Err(error) = fs.remove(&expanded)
                {
                    return Err(Error::Plan(format!(
                        "cannot remove link '{}': {error}",
                        expanded.display()
                    )));
                }
                copy_blob(blob, blobs, fs, &expanded)
            }
            _ => {
                if fs.read_link(&expanded).is_some()
                    && let Err(error) = fs.remove(&expanded)
                {
                    return Err(Error::Plan(format!(
                        "cannot remove link '{}': {error}",
                        expanded.display()
                    )));
                }
                let bytes = document.render(blobs)?;
                fs.write(&expanded, &bytes).map_err(Error::from)
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
        written += 1;
    }
    Ok(written)
}

/// Streams one blob to its destination through the read source.
///
/// Pool sources hold gzip bytes, so they decode through a
/// stream into the destination. Ref files hold raw bytes, so
/// they copy straight across.
///
/// # Arguments
///
/// * `sha` - the SHA-256 hex over raw content bytes.
/// * `blobs` - the blob refs under content hashes.
/// * `fs` - the backend under copying.
/// * `dest` - the expanded destination file.
///
/// # Returns
///
/// Unit once the bytes land.
///
/// # Errors
///
/// Dangling hashes, unreadable sources, and unwritable
/// destinations fail as plan or io errors.
fn copy_blob(
    sha: &str,
    blobs: &BTreeMap<String, BlobRef>,
    fs: &dyn Filesystem,
    dest: &std::path::Path,
) -> Result<()> {
    match blob_source(sha, blobs, fs)? {
        BlobSource::Pool(path) => {
            use std::io::Write as _;

            let reader = fs.reader(&path)?;
            let mut decoder = flate2::read::GzDecoder::new(reader);
            let mut writer = fs.writer(dest)?;
            std::io::copy(&mut decoder, &mut writer)?;
            writer.flush()?;
            Ok(())
        }
        BlobSource::File(path) => fs.copy(&path, dest).map(|_| ()).map_err(Error::from),
    }
}

/// Writes one tree destination member by member.
///
/// Only the members land, never the destination folder
/// itself. Parent folders create as needed, modes land
/// per member from the manifest. Member bytes resolve
/// pool-first through their refs, then stream to disk.
///
/// # Arguments
///
/// * `dest` - the expanded destination folder.
/// * `members` - the desired members under writing.
/// * `blobs` - the blob refs under content hashes.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once every member lands.
///
/// # Errors
///
/// Missing refs, unreadable sources, unwritable
/// destinations and mode failures surface as plan or io
/// errors carrying the member path.
fn write_tree_members(
    dest: &std::path::Path,
    members: &[ManifestMember],
    blobs: &BTreeMap<String, BlobRef>,
    fs: &dyn Filesystem,
) -> Result<()> {
    for member in members {
        let path = dest.join(&member.relative);
        copy_blob(&member.blob, blobs, fs, &path).map_err(|error| match error {
            Error::Plan(_) => Error::Plan(format!(
                "missing blob '{}' for '{}'",
                member.blob,
                path.display()
            )),
            Error::Io(error) => Error::Io(std::io::Error::new(
                error.kind(),
                format!("cannot write '{}': {error}", path.display()),
            )),
        })?;
        fs.set_mode(&path, member.mode).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!("cannot set mode '{}': {error}", path.display()),
            ))
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

/// Removes dropped tree members between recorded and desired manifests.
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

    /// Builds spill refs for raw bytes, mirroring scratch into the memory backend.
    ///
    /// Spill files live outside the managed filesystem, so the
    /// mirror lets ref paths resolve through the fake.
    fn ref_map(fs: &crate::fs::memory::MemoryFs, pairs: &[&[u8]]) -> BTreeMap<String, BlobRef> {
        use crate::fs::Filesystem as _;

        let mut out = BTreeMap::new();
        for bytes in pairs {
            let path = crate::store::blobs::spill_bytes(bytes);
            if let Err(error) = fs.write(&path, bytes) {
                panic!("spill mirrors: {error}");
            }
            let sha = crate::ids::sha256_hex(bytes);
            out.insert(
                sha.clone(),
                BlobRef {
                    sha,
                    size: bytes.len() as u64,
                    path,
                },
            );
        }
        out
    }

    fn tree_recorded() -> Vec<ManifestDocument> {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::ids::DocPath;

        vec![ManifestDocument::new(
            DocPath::new("fonts"),
            ManifestData::Tree {
                members: vec![
                    ManifestMember {
                        relative: "kept.ttf".into(),
                        blob: crate::ids::sha256_hex(&[1]),
                        size: 1,
                        mode: 0o644,
                    },
                    ManifestMember {
                        relative: "gone.ttf".into(),
                        blob: crate::ids::sha256_hex(&[2]),
                        size: 1,
                        mode: 0o644,
                    },
                ],
            },
        )]
    }

    fn tree_refs(fs: &crate::fs::memory::MemoryFs) -> BTreeMap<String, BlobRef> {
        ref_map(fs, &[&[1], &[2]])
    }

    #[test]
    fn write_tree_members_land_with_modes() {
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        assert!(
            write_documents(
                &tree_recorded(),
                &tree_refs(&fs),
                &fs,
                None,
                &BTreeSet::new()
            )
            .is_ok()
        );
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
        use crate::fs::{Filesystem, memory::MemoryFs};
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
                unmanaged: false,
            },
        )];
        assert!(
            write_documents(
                &documents,
                &std::collections::BTreeMap::new(),
                &fs,
                None,
                &BTreeSet::new()
            )
            .is_ok()
        );
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
    fn unmanaged_present_skips_write_keeping_bytes() {
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        assert!(fs.write(std::path::Path::new("bin"), b"old").is_ok());
        let blob = crate::ids::sha256_hex(b"new");
        let documents = vec![ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: blob.clone(),
                size: 3,
                mode: None,
                unmanaged: true,
            },
        )];
        let blobs = ref_map(&fs, &[b"new"]);
        assert!(blobs.contains_key(&blob));
        match write_documents(&documents, &blobs, &fs, None, &BTreeSet::new()) {
            Ok(written) => assert_eq!(written, 0),
            Err(error) => panic!("unmanaged skips: {error}"),
        }
        match fs.read(std::path::Path::new("bin")) {
            Ok(bytes) => assert_eq!(bytes, b"old"),
            Err(error) => panic!("target reads: {error}"),
        }
    }

    #[test]
    fn unmanaged_absent_writes_declared_bytes() {
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        let blob = crate::ids::sha256_hex(b"fresh");
        let documents = vec![ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: blob.clone(),
                size: 5,
                mode: None,
                unmanaged: true,
            },
        )];
        let blobs = ref_map(&fs, &[b"fresh"]);
        assert!(blobs.contains_key(&blob));
        match write_documents(&documents, &blobs, &fs, None, &BTreeSet::new()) {
            Ok(written) => assert_eq!(written, 1),
            Err(error) => panic!("unmanaged creates: {error}"),
        }
        match fs.read(std::path::Path::new("bin")) {
            Ok(bytes) => assert_eq!(bytes, b"fresh"),
            Err(error) => panic!("fresh reads: {error}"),
        }

        // Pool refs decode pooled gzip bytes while their
        // materialized path stays missing.
        let pooled = b"pooled";
        let pooled_sha = crate::ids::sha256_hex(pooled);
        let pool = match crate::store::blobs::resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        let gzipped = match crate::store::blobs::gzip_bytes(pooled) {
            Ok(gzipped) => gzipped,
            Err(error) => panic!("blob compresses: {error}"),
        };
        if let Err(error) = fs.write(&pool.join(&pooled_sha), &gzipped) {
            panic!("pool seeds: {error}");
        }
        let pooled_documents = vec![ManifestDocument::new(
            DocPath::new("pooled-bin"),
            ManifestData::Opaque {
                blob: pooled_sha.clone(),
                size: pooled.len() as u64,
                mode: None,
                unmanaged: true,
            },
        )];
        let pooled_blobs = BTreeMap::from([(
            pooled_sha,
            BlobRef {
                sha: crate::ids::sha256_hex(pooled),
                size: pooled.len() as u64,
                path: std::path::PathBuf::from("nowhere/missing.bin"),
            },
        )]);
        match write_documents(
            &pooled_documents,
            &pooled_blobs,
            &fs,
            None,
            &BTreeSet::new(),
        ) {
            Ok(written) => assert_eq!(written, 1),
            Err(error) => panic!("pooled writes: {error}"),
        }
        match fs.read(std::path::Path::new("pooled-bin")) {
            Ok(bytes) => assert_eq!(bytes, pooled),
            Err(error) => panic!("pooled reads: {error}"),
        }

        // Dangling refs fail naming the hash.
        let dangling = "0".repeat(64);
        let dangling_documents = vec![ManifestDocument::new(
            DocPath::new("dangling-bin"),
            ManifestData::Opaque {
                blob: dangling.clone(),
                size: 1,
                mode: None,
                unmanaged: true,
            },
        )];
        match write_documents(
            &dangling_documents,
            &BTreeMap::new(),
            &fs,
            None,
            &BTreeSet::new(),
        ) {
            Ok(_) => panic!("dangling ref passes"),
            Err(error) => assert!(
                error.to_string().contains(&dangling),
                "error names the hash: {error}"
            ),
        }
    }

    #[test]
    fn written_count_drops_skipped_docs() {
        use crate::document::{ManifestData, ManifestDocument};
        use crate::fs::{Filesystem, memory::MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        assert!(fs.write(std::path::Path::new("bin"), b"old").is_ok());
        let kept_blob = crate::ids::sha256_hex(b"new");
        let fresh_blob = crate::ids::sha256_hex(b"fresh");
        let documents = vec![
            ManifestDocument::new(
                DocPath::new("note"),
                ManifestData::Text {
                    content: "hi".into(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("bin"),
                ManifestData::Opaque {
                    blob: kept_blob.clone(),
                    size: 3,
                    mode: None,
                    unmanaged: true,
                },
            ),
            ManifestDocument::new(
                DocPath::new("tool"),
                ManifestData::Opaque {
                    blob: fresh_blob.clone(),
                    size: 5,
                    mode: None,
                    unmanaged: true,
                },
            ),
        ];
        let blobs = ref_map(&fs, &[b"new", b"fresh"]);
        assert!(blobs.contains_key(&kept_blob));
        assert!(blobs.contains_key(&fresh_blob));
        match write_documents(&documents, &blobs, &fs, None, &BTreeSet::new()) {
            Ok(written) => assert_eq!(written, 2),
            Err(error) => panic!("mixed writes: {error}"),
        }
        match fs.read(std::path::Path::new("bin")) {
            Ok(bytes) => assert_eq!(bytes, b"old"),
            Err(error) => panic!("skipped reads: {error}"),
        }
        assert!(fs.exists(std::path::Path::new("note")));
        assert!(fs.exists(std::path::Path::new("tool")));
    }

    #[test]
    fn unmanaged_present_rewrites_once_on_declaration_change() {
        use crate::fs::{Filesystem, memory::MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        assert!(fs.write(std::path::Path::new("bin"), b"old").is_ok());
        let blob = crate::ids::sha256_hex(b"new");
        let documents = vec![ManifestDocument::new(
            DocPath::new("bin"),
            ManifestData::Opaque {
                blob: blob.clone(),
                size: 3,
                mode: None,
                unmanaged: true,
            },
        )];
        let blobs = ref_map(&fs, &[b"new"]);
        assert!(blobs.contains_key(&blob));
        let rewritten = BTreeSet::from([DocPath::new("bin")]);
        match write_documents(&documents, &blobs, &fs, None, &rewritten) {
            Ok(written) => assert_eq!(written, 1),
            Err(error) => panic!("unmanaged rewrites: {error}"),
        }
        match fs.read(std::path::Path::new("bin")) {
            Ok(bytes) => assert_eq!(bytes, b"new"),
            Err(error) => panic!("rewritten reads: {error}"),
        }
    }

    #[test]
    fn unmanaged_text_stays_quiet_while_present() {
        use crate::document::{ManifestData, ManifestDocument};
        use crate::fs::{Filesystem, memory::MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        assert!(
            fs.write(std::path::Path::new("note"), b"hand-edited")
                .is_ok()
        );
        let documents = vec![ManifestDocument::new(
            DocPath::new("note"),
            ManifestData::Text {
                content: "declared".into(),
                mode: None,
                unmanaged: true,
            },
        )];
        match write_documents(&documents, &BTreeMap::new(), &fs, None, &BTreeSet::new()) {
            Ok(written) => assert_eq!(written, 0),
            Err(error) => panic!("unmanaged text skips: {error}"),
        }
        match fs.read(std::path::Path::new("note")) {
            Ok(bytes) => assert_eq!(bytes, b"hand-edited"),
            Err(error) => panic!("kept reads: {error}"),
        }
    }

    #[test]
    fn remove_tree_members_drops_only_dropped() {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::fs::{Filesystem, memory::MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        assert!(
            write_documents(
                &tree_recorded(),
                &tree_refs(&fs),
                &fs,
                None,
                &BTreeSet::new()
            )
            .is_ok()
        );
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
                    blob: crate::ids::sha256_hex(&[1]),
                    size: 1,
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
        use crate::fs::memory::MemoryFs;

        let fs = MemoryFs::new();
        assert!(
            write_documents(
                &tree_recorded(),
                &tree_refs(&fs),
                &fs,
                None,
                &BTreeSet::new()
            )
            .is_ok()
        );
        match remove_orphans(&tree_recorded(), &[], &fs) {
            Ok(removed) => assert_eq!(removed, 0),
            Err(error) => panic!("orphans remove: {error}"),
        }
        let dest = std::path::Path::new("fonts");
        assert!(fs.exists(&dest.join("kept.ttf")));
    }
}
