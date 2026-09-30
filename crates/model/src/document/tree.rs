//! Tree
//!
//! Managed file sets with lifecycle counts and hashing.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{BlobRef, DocumentStatus};
use crate::manifest::Manifest;

/// One persisted tree member holding a blob ref.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestMember {
    /// Holds the destination-relative member path.
    pub relative: String,
    /// Holds the content-addressed member bytes identity.
    pub blob: BlobRef,
    /// Holds unix permission bits for the member file.
    pub mode: u32,
}

/// Lifecycle counts for one bundle against a previous manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Summary {
    /// Counts documents absent from the previous manifest.
    pub create: usize,
    /// Counts documents with a differing data hash.
    pub update: usize,
    /// Counts previous documents missing from the manifest.
    pub delete: usize,
}

/// Counts lifecycle states against a previous manifest.
///
/// Opaque kind changes count as updates, other kind changes
/// count as create and delete.
///
/// # Examples
///
/// ```rust
/// use confit_model::document::{Data, Document, summary};
/// use confit_model::manifest::{MANIFEST_VERSION, Manifest};
/// use confit_model::routes::{Route, RouteBase};
///
/// let mut previous = Manifest {
///     version: MANIFEST_VERSION,
///     documents: vec![Document::new(
///         Route::new(RouteBase::Home, "note").unwrap(),
///         Data::Text { content: "hi".into(), mode: None, unmanaged: false },
///     )],
///     hooks: Vec::new(),
/// };
/// for document in &mut previous.documents {
///     document.fill_hash().unwrap();
/// }
/// let mut built_docs = vec![Document::new(
///     Route::new(RouteBase::Home, "note").unwrap(),
///     Data::Text { content: "changed".into(), mode: None, unmanaged: false },
/// )];
/// for document in &mut built_docs {
///     document.fill_hash().unwrap();
/// }
/// let built = Manifest {
///     version: MANIFEST_VERSION,
///     documents: built_docs,
///     hooks: Vec::new(),
/// };
/// assert_eq!(summary(&built, &previous).update, 1);
/// ```
pub fn summary(built: &Manifest, previous: &Manifest) -> Summary {
    let mut counts = Summary {
        create: 0,
        update: 0,
        delete: 0,
    };
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for document in &built.documents {
        seen.insert(document.key());
        match document.status(previous) {
            DocumentStatus::Create => counts.create += 1,
            DocumentStatus::Update => counts.update += 1,
            DocumentStatus::Unchanged => {}
        }
    }
    for recorded in &previous.documents {
        if !seen.contains(&recorded.key()) && !recorded.superseded_by(&built.documents) {
            counts.delete += 1;
        }
    }
    counts
}

/// Counts changed members between two tree manifests.
///
/// # Examples
///
/// ```rust
/// use confit_model::document::{BlobRef, ManifestMember, tree_changed};
/// use confit_model::sha::Sha;
///
/// fn sealed(content: String, stored: String) -> BlobRef {
///     BlobRef::new(Sha::new(content).unwrap(), Sha::new(stored).unwrap())
/// }
///
/// let old = vec![ManifestMember { relative: "a".into(), blob: sealed("aa".repeat(32), "aa".repeat(32)), mode: 0o644 }];
/// let new = vec![
///     ManifestMember { relative: "a".into(), blob: sealed("bb".repeat(32), "bb".repeat(32)), mode: 0o644 },
///     ManifestMember { relative: "b".into(), blob: sealed("cc".repeat(32), "cc".repeat(32)), mode: 0o644 },
/// ];
/// assert_eq!(tree_changed(&old, &new), 2);
/// ```
pub fn tree_changed(old: &[ManifestMember], new: &[ManifestMember]) -> usize {
    let old_map: BTreeMap<&str, &ManifestMember> = old
        .iter()
        .map(|member| (member.relative.as_str(), member))
        .collect();
    let new_map: BTreeMap<&str, &ManifestMember> = new
        .iter()
        .map(|member| (member.relative.as_str(), member))
        .collect();
    let mut changed = 0;
    for (relative, member) in &new_map {
        match old_map.get(relative) {
            Some(previous) if previous.blob == member.blob && previous.mode == member.mode => {
                continue;
            }
            _ => changed += 1,
        }
    }
    for rel in old_map.keys() {
        if !new_map.contains_key(rel) {
            changed += 1;
        }
    }
    changed
}

/// Renders the canonical manifest bytes for tree hashing.
pub(crate) fn tree_manifest_bytes(members: &[ManifestMember]) -> Vec<u8> {
    let mut sorted: Vec<&ManifestMember> = members.iter().collect();
    sorted.sort_by(|left, right| left.relative.cmp(&right.relative));
    let mut out = Vec::new();
    for member in sorted {
        out.extend_from_slice(
            format!(
                "{:o} {} {}\n",
                member.mode,
                member.relative,
                member.blob.sha()
            )
            .as_bytes(),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Data, Document};
    use crate::manifest::MANIFEST_VERSION;
    use crate::routes::{Route, RouteBase};
    use crate::sha::Sha;

    fn sealed(content: &str, stored: &str) -> BlobRef {
        BlobRef::new(Sha::new(content).unwrap(), Sha::new(stored).unwrap())
    }

    fn text_doc(name: &str, content: &str) -> Document {
        let mut document = Document::new(
            Route::new(RouteBase::Home, name).unwrap(),
            Data::Text {
                content: content.into(),
                mode: None,
                unmanaged: false,
            },
        );
        document.fill_hash().unwrap();
        document
    }

    #[test]
    fn summary_counts_create_and_delete() {
        let built = Manifest {
            version: MANIFEST_VERSION,
            documents: vec![text_doc("fresh", "hi")],
            hooks: Vec::new(),
        };
        let previous = Manifest {
            version: MANIFEST_VERSION,
            documents: vec![text_doc("stale", "hi")],
            hooks: Vec::new(),
        };
        let counts = summary(&built, &previous);
        assert_eq!(
            (counts.create, counts.update, counts.delete),
            (1, 0, 1),
            "fresh reads as create and stale reads as delete"
        );
    }

    #[test]
    fn tree_changed_counts_mode_change_and_removal() {
        let blob = sealed(&"aa".repeat(32), &"bb".repeat(32));
        let old = vec![
            ManifestMember {
                relative: "a".into(),
                blob: blob.clone(),
                mode: 0o644,
            },
            ManifestMember {
                relative: "b".into(),
                blob: blob.clone(),
                mode: 0o644,
            },
        ];
        let new = vec![ManifestMember {
            relative: "a".into(),
            blob,
            mode: 0o755,
        }];
        assert_eq!(
            tree_changed(&old, &new),
            2,
            "mode edit and missing member each count once"
        );
    }
}
