//! Plan
//!
//! Desired state builds with two comparisons.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::{ManifestData, ManifestDocument};
use crate::error::Result;
use crate::fs::Filesystem;
use crate::hook::{Hook, preview_hook};
use crate::ids::{DocPath, sha256_hex};
use crate::runtime::Runtime;
use crate::store::blobs::BlobRef;
use crate::store::manifest::Manifest;

/// Bundle format version written by every bundle build.
///
pub const BUNDLE_VERSION: u32 = 7;

/// Versioned desired state written by bundle builds.
///
/// The manifest holds version, documents, and
/// hooks as the only document language. The blob map holds
/// blob refs under content hashes beside it. The bundle
/// holds no duplicate fields.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Holds the portable manifest as the only document language.
    pub manifest: Manifest,
    /// Holds blob refs under SHA-256 hex hashes.
    pub blobs: BTreeMap<String, BlobRef>,
}

impl Bundle {
    /// Builds an empty manifest with the current version.
    ///
    /// # Returns
    ///
    /// The bundle holding version and empty documents.
    ///
    pub fn empty() -> Self {
        Self {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents: Vec::new(),
                hooks: Vec::new(),
            },
            blobs: BTreeMap::new(),
        }
    }

    /// Finds one recorded document by its kind and path key.
    fn find_by_key(&self, key: &str) -> Option<&ManifestDocument> {
        self.manifest
            .documents
            .iter()
            .find(|document| document.key() == key)
    }

    /// Finds one recorded document sharing path with opaque kind.
    fn find_same_path_opaque(&self, document: &ManifestDocument) -> Option<&ManifestDocument> {
        self.manifest.documents.iter().find(|recorded| {
            recorded.path == document.path
                && recorded.key() != document.key()
                && (recorded.is_opaque() || document.is_opaque())
        })
    }
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

/// Per-document lifecycle status against previous manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// Document absent from previous manifest.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

impl ManifestDocument {
    /// Reports the lifecycle status against a previous manifest.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous manifest with filled hashes.
    ///
    /// # Returns
    ///
    /// Create for absent keys, update for differing hashes,
    /// differing modes, or opaque kind changes, else unchanged.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::{DocumentStatus, Bundle};
    ///
    /// let mut document = ManifestDocument::new(
    ///     DocPath::new("x"),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.status(&Bundle::empty()), DocumentStatus::Create));
    /// ```
    pub fn status(&self, previous: &Bundle) -> DocumentStatus {
        match previous.find_by_key(&self.key()) {
            None => match previous.find_same_path_opaque(self) {
                Some(_) => DocumentStatus::Update,
                None => DocumentStatus::Create,
            },
            Some(recorded)
                if recorded.data_hash == self.data_hash && recorded.mode() == self.mode() =>
            {
                DocumentStatus::Unchanged
            }
            Some(_) => DocumentStatus::Update,
        }
    }

    /// Fills the data hash by rendering the document.
    ///
    /// The hash covers rendered bytes only. Modes compare
    /// separately through status and drift. Opaque hashes
    /// copy the blob reference, since the blob holds the
    /// SHA-256 over raw bytes. Tree hashes cover canonical
    /// manifest bytes over blob references.
    ///
    /// # Returns
    ///
    /// Unit once the hash fills.
    ///
    /// # Errors
    ///
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::ids::DocPath;
    ///
    /// let mut document = ManifestDocument::new(
    ///     DocPath::new("x"),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.data_hash.is_empty(), false));
    /// ```
    pub fn fill_hash(&mut self) -> Result<()> {
        match &self.data {
            ManifestData::Opaque { blob, .. } => {
                self.data_hash = blob.clone();
                Ok(())
            }
            ManifestData::Tree { members } => {
                self.data_hash = sha256_hex(&crate::document::tree_manifest_bytes(members));
                Ok(())
            }
            inline => {
                let bytes = crate::render::render_inline_bytes(inline)?;
                self.data_hash = sha256_hex(&bytes);
                Ok(())
            }
        }
    }

    /// Reports whether a recorded document yields to desired documents.
    ///
    /// A recorded key yields while some desired document shares
    /// its path under another key with either side opaque.
    ///
    /// # Arguments
    ///
    /// * `desired` - the desired documents under comparing.
    ///
    /// # Returns
    ///
    /// True while an opaque same-path sibling exists in desired.
    ///
    pub fn superseded_by(&self, desired: &[ManifestDocument]) -> bool {
        desired.iter().any(|document| {
            document.path == self.path
                && document.key() != self.key()
                && (document.is_opaque() || self.is_opaque())
        })
    }
}

/// Reads the hash and size label for opaque bytes.
///
/// # Arguments
///
/// * `bytes` - the raw bytes under labeling.
///
/// # Returns
///
/// The `sha256:{hex} ({n} bytes)` label.
///
/// # Examples
///
/// ```rust
/// use confit_core::plan::opaque_label;
///
/// let label = opaque_label(&[0xFF, 0x00]);
/// assert!(matches!(label.starts_with("sha256:"), true));
/// assert!(matches!(label.contains("(2 bytes)"), true));
/// ```
pub fn opaque_label(bytes: &[u8]) -> String {
    format!("sha256:{} ({} bytes)", sha256_hex(bytes), bytes.len())
}

impl Bundle {
    /// Builds the desired state bundle from documents.
    ///
    /// Fills data hashes, then sorts documents by path. The
    /// caller holds one document per path. Blob refs ride
    /// beside the manifest and fill during hydration. Counts
    /// generate through `summary` against a previous manifest.
    ///
    /// # Arguments
    ///
    /// * `documents` - desired documents in pipeline order, unique per path.
    /// * `hooks` - desired hooks in declaration order, merged downstream.
    ///
    /// # Returns
    ///
    /// The built bundle.
    ///
    /// # Errors
    ///
    /// Serializer failures fail as plan errors.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::Bundle;
    ///
    /// let document = ManifestDocument::new(
    ///     DocPath::new("note"),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// let outcome = Bundle::build(vec![document], Vec::new());
    /// let previous = Bundle::empty();
    /// assert!(matches!(outcome, Ok(bundle) if bundle.summary(&previous).create == 1));
    /// ```
    pub fn build(mut documents: Vec<ManifestDocument>, hooks: Vec<Hook>) -> Result<Self> {
        for document in &mut documents {
            document.fill_hash()?;
        }
        documents.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(Self {
            manifest: Manifest {
                version: BUNDLE_VERSION,
                documents,
                hooks,
            },
            blobs: BTreeMap::new(),
        })
    }

    /// Counts lifecycle states against a previous manifest.
    ///
    /// Opaque kind changes count as updates, other kind changes
    /// count as create and delete.
    ///
    /// # Arguments
    ///
    /// * `previous` - the previous manifest with filled hashes.
    ///
    /// # Returns
    ///
    /// Create, update, and delete counts.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::ids::DocPath;
    /// use confit_core::plan::Bundle;
    ///
    /// let mut previous = Bundle::empty();
    /// previous.manifest.documents = vec![ManifestDocument::new(
    ///     DocPath::new("note"),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// )];
    /// let bundle = Bundle::build(
    ///     vec![ManifestDocument::new(
    ///         DocPath::new("note"),
    ///         ManifestData::Text { content: "changed".into(), mode: None, unmanaged: false},
    ///     )],
    ///     Vec::new(),
    /// );
    /// assert!(matches!(bundle, Ok(bundle) if bundle.summary(&previous).update == 1));
    /// ```
    pub fn summary(&self, previous: &Bundle) -> Summary {
        let mut summary = Summary {
            create: 0,
            update: 0,
            delete: 0,
        };
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for document in &self.manifest.documents {
            seen.insert(document.key());
            match document.status(previous) {
                DocumentStatus::Create => summary.create += 1,
                DocumentStatus::Update => summary.update += 1,
                DocumentStatus::Unchanged => {}
            }
        }
        for recorded in &previous.manifest.documents {
            if !seen.contains(&recorded.key()) && !recorded.superseded_by(&self.manifest.documents)
            {
                summary.delete += 1;
            }
        }
        summary
    }

    /// Renders one preview line per hook in plan order.
    ///
    /// # Arguments
    ///
    /// * `rt` - the runtime facts under reading.
    /// * `fs` - the backend under stating.
    /// * `changed` - the changed document ids under reading.
    ///
    /// # Returns
    ///
    /// The preview lines in plan order.
    ///
    /// # Errors
    ///
    /// Unresolvable binaries fail as plan errors naming the hook.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::plan::Bundle;
    /// use confit_core::fs::memory::MemoryFs;
    /// use confit_core::runtime::Runtime;
    /// use std::collections::BTreeSet;
    ///
    /// let rt = Runtime { vars: Default::default(), path_dirs: Vec::new() };
    /// let lines = Bundle::empty().hook_preview(&rt, &MemoryFs::new(), &BTreeSet::new());
    /// assert!(matches!(lines, Ok(lines) if lines.is_empty()));
    /// ```
    pub fn hook_preview(
        &self,
        rt: &Runtime,
        fs: &dyn Filesystem,
        changed: &BTreeSet<DocPath>,
    ) -> Result<Vec<String>> {
        let mut lines = Vec::with_capacity(self.manifest.hooks.len());
        for hook in &self.manifest.hooks {
            lines.push(preview_hook(hook, rt, fs, changed)?);
        }
        Ok(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{ManifestData, ManifestDocument};
    use crate::ids::DocPath;

    fn text_doc(path: &str, content: &str) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Text {
                content: content.to_string(),
                mode: None,
                unmanaged: false,
            },
        )
    }

    fn with_hashes(documents: Vec<ManifestDocument>) -> Bundle {
        let mut docs = documents;
        for document in &mut docs {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let mut previous = Bundle::empty();
        previous.manifest.documents = docs;
        previous
    }

    #[test]
    fn plan_counts_create_update_delete() {
        let previous = with_hashes(vec![text_doc("a", "same-a"), text_doc("gone", "gone")]);
        let stale = previous.manifest.documents[0].clone();
        let desired = vec![text_doc("a", "same-a"), text_doc("b", "fresh-b")];
        let _ = stale;
        let outcome = Bundle::build(desired, Vec::new());
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.create, 1);
        assert_eq!(summary.update, 0);
        assert_eq!(summary.delete, 1);
        assert!(matches!(
            built.manifest.documents[0].status(&previous),
            DocumentStatus::Unchanged
        ));
        assert!(matches!(
            built.manifest.documents[1].status(&previous),
            DocumentStatus::Create
        ));
    }

    #[test]
    fn plan_marks_update_on_hash_change() {
        let previous = with_hashes(vec![text_doc("b", "old")]);
        let outcome = Bundle::build(vec![text_doc("b", "new")], Vec::new());
        let built = match outcome {
            Ok(out) => out,
            Err(error) => panic!("bundle builds: {error}"),
        };
        assert_eq!(built.summary(&previous).update, 1);
        assert!(matches!(
            built.manifest.documents[0].status(&previous),
            DocumentStatus::Update
        ));
    }

    fn opaque_doc(path: &str, bytes: &[u8]) -> ManifestDocument {
        ManifestDocument::new(
            DocPath::new(path),
            ManifestData::Opaque {
                blob: sha256_hex(bytes),
                size: bytes.len() as u64,
                mode: None,
                unmanaged: false,
            },
        )
    }

    #[test]
    fn opaque_kind_change_reads_as_update_both_ways() {
        let previous = with_hashes(vec![text_doc("bin", "hi")]);
        let desired = opaque_doc("bin", &[0xFF, 0x00]);
        let mut hashed = vec![desired.clone()];
        for document in &mut hashed {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        assert!(matches!(
            hashed[0].status(&previous),
            DocumentStatus::Update
        ));
        let previous_opaque = with_hashes(vec![opaque_doc("bin", &[0xFF, 0x00])]);
        let mut back = vec![text_doc("bin", "hi")];
        for document in &mut back {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        assert!(matches!(
            back[0].status(&previous_opaque),
            DocumentStatus::Update
        ));
    }

    #[test]
    fn opaque_kind_change_skips_superseded_delete() {
        let previous = with_hashes(vec![text_doc("bin", "hi")]);
        let mut desired = vec![opaque_doc("bin", &[0xFF, 0x00])];
        for document in &mut desired {
            if let Err(error) = document.fill_hash() {
                panic!("hashes fill: {error}");
            }
        }
        let built = match Bundle::build(vec![opaque_doc("bin", &[0xFF, 0x00])], Vec::new()) {
            Ok(out) => out,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.update, 1);
        assert_eq!(summary.delete, 0);
        assert_eq!(summary.create, 0);
    }
    #[test]
    fn plain_kind_change_keeps_create_plus_delete() {
        let previous = with_hashes(vec![text_doc("bin", "hi")]);
        let built = match Bundle::build(
            vec![ManifestDocument::new(
                DocPath::new("bin"),
                ManifestData::Link {
                    target: "dest".to_string(),
                },
            )],
            Vec::new(),
        ) {
            Ok(out) => out,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let summary = built.summary(&previous);
        assert_eq!(summary.create, 1);
        assert_eq!(summary.delete, 1);
        assert_eq!(summary.update, 0);
    }
    #[test]
    fn build_carries_hooks_through() {
        use crate::hook::Hook;

        let hooks = vec![Hook {
            argv: vec!["mise".to_string()],
            path: Vec::new(),
            requires: None,
            when: None,
            checks: Vec::new(),
            timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }];
        let built = match Bundle::build(Vec::new(), hooks) {
            Ok(out) => out,
            Err(error) => panic!("bundle builds: {error}"),
        };
        assert_eq!(built.manifest.version, BUNDLE_VERSION);
        assert_eq!(built.manifest.hooks.len(), 1);
        assert_eq!(built.manifest.hooks[0].argv, vec!["mise".to_string()]);
    }

    fn preview_runtime() -> (crate::runtime::Runtime, crate::fs::memory::MemoryFs) {
        use crate::fs::Filesystem;

        let fs = crate::fs::memory::MemoryFs::new();
        let _ = fs.write(std::path::Path::new("/opt/tool"), b"run");
        let _ = fs.set_mode(std::path::Path::new("/opt/tool"), 0o755);
        let _ = fs.write(std::path::Path::new("/opt/probe"), b"run");
        let rt = crate::runtime::Runtime {
            vars: std::collections::BTreeMap::new(),
            path_dirs: vec![std::path::PathBuf::from("/opt")],
        };
        (rt, fs)
    }

    fn preview_hook(argv: &[&str]) -> crate::hook::Hook {
        crate::hook::Hook {
            argv: argv.iter().map(|item| item.to_string()).collect(),
            path: Vec::new(),
            requires: None,
            when: None,
            checks: Vec::new(),
            timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }
    }

    #[test]
    fn hook_preview_renders_run_skip_warn_lines() {
        use crate::condition::Condition;

        let (rt, fs) = preview_runtime();
        let mut bundle = Bundle::empty();
        bundle.manifest.hooks = vec![
            preview_hook(&["tool", "--flag"]),
            crate::hook::Hook {
                checks: vec![Condition::Exists {
                    path: "/opt/probe".into(),
                }],
                ..preview_hook(&["tool"])
            },
            crate::hook::Hook {
                when: Some(Condition::InPath {
                    name: "absent".into(),
                }),
                ..preview_hook(&["tool"])
            },
        ];
        let lines = match bundle.hook_preview(&rt, &fs, &BTreeSet::new()) {
            Ok(lines) => lines,
            Err(error) => panic!("preview renders: {error}"),
        };
        assert_eq!(
            lines,
            vec![
                "! run: /opt/tool --flag".to_string(),
                "skipped: tool (checks pass)".to_string(),
                "skipped: tool (no need: in_path(absent))".to_string(),
            ]
        );
    }

    #[test]
    fn hook_preview_runs_on_failing_checks() {
        use crate::condition::Condition;

        let (rt, fs) = preview_runtime();
        let mut bundle = Bundle::empty();
        bundle.manifest.hooks = vec![crate::hook::Hook {
            checks: vec![Condition::Exists {
                path: "/opt/absent".into(),
            }],
            ..preview_hook(&["tool"])
        }];
        let lines = match bundle.hook_preview(&rt, &fs, &BTreeSet::new()) {
            Ok(lines) => lines,
            Err(error) => panic!("preview renders: {error}"),
        };
        assert_eq!(lines, vec!["! run: /opt/tool".to_string()]);
    }

    #[test]
    fn hook_preview_miss_fails_naming_hook() {
        let (rt, fs) = preview_runtime();
        let mut bundle = Bundle::empty();
        bundle.manifest.hooks = vec![preview_hook(&["absent", "install"])];
        match bundle.hook_preview(&rt, &fs, &BTreeSet::new()) {
            Ok(_) => panic!("missing binary passes"),
            Err(error) => assert_eq!(
                error.to_string(),
                "hook 'absent install' cannot resolve 'absent'"
            ),
        }
    }
}
