//! Plan
//!
//! Desired state builds with two comparisons.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::arg::Arg;
use crate::document::{ManifestData, ManifestDocument};
use crate::error::Result;
use crate::handles::BlobHandle;
use crate::hook::Hook;
use crate::ids::sha256_hex;
use crate::store::manifest::Manifest;

/// Bundle format version written by every bundle build.
///
pub const BUNDLE_VERSION: u32 = 7;

/// Versioned desired state written by bundle builds.
///
/// The manifest holds version, documents, and
/// hooks as the only document language. The blob map holds
/// blob handles under content hashes beside it. Secret
/// documents hold no hashes here; their bytes arrive at
/// apply time. The bundle holds no duplicate fields.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Holds the portable manifest as the only document language.
    pub manifest: Manifest,
    /// Holds blob handles under SHA-256 hex hashes.
    pub blobs: BTreeMap<String, BlobHandle>,
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

    /// Finds one recorded document sharing destination with opaque kind.
    fn find_same_path_opaque(&self, document: &ManifestDocument) -> Option<&ManifestDocument> {
        self.manifest.documents.iter().find(|recorded| {
            recorded.destination == document.destination
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
    /// use confit_core::handles::{Route, RouteBase};
    /// use confit_core::plan::{DocumentStatus, Bundle};
    ///
    /// let mut document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
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
    /// copy the blob handle, since the handle is the
    /// SHA-256 over raw bytes. Tree hashes cover canonical
    /// manifest bytes over blob handles. Secret hashes
    /// cover the command argv, since secret bytes arrive
    /// at apply time alone.
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
    /// use confit_core::handles::{Route, RouteBase};
    ///
    /// let mut document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.data_hash.is_empty(), false));
    /// ```
    pub fn fill_hash(&mut self) -> Result<()> {
        match &self.data {
            ManifestData::Opaque { blob, .. } => {
                self.data_hash = blob.sha().to_string();
                Ok(())
            }
            ManifestData::Tree { members } => {
                self.data_hash = sha256_hex(&crate::document::tree_manifest_bytes(members));
                Ok(())
            }
            ManifestData::Secret { argv, .. } => {
                let joined = argv.iter().map(Arg::display).collect::<Vec<_>>().join("\0");
                self.data_hash = sha256_hex(joined.as_bytes());
                Ok(())
            }
            inline => {
                let bytes = crate::render::render_inline_bytes(inline, &|route| {
                    PathBuf::from(route.display())
                })?;
                self.data_hash = sha256_hex(&bytes);
                Ok(())
            }
        }
    }

    /// Reports whether a recorded document yields to desired documents.
    ///
    /// A recorded key yields while some desired document shares
    /// its destination under another key with either side opaque.
    ///
    /// # Arguments
    ///
    /// * `desired` - the desired documents under comparing.
    ///
    /// # Returns
    ///
    /// True while an opaque same-destination sibling exists in desired.
    ///
    pub fn superseded_by(&self, desired: &[ManifestDocument]) -> bool {
        desired.iter().any(|document| {
            document.destination == self.destination
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
    /// Fills data hashes, then sorts documents by destination.
    /// The caller holds one document per destination. Secret
    /// payloads hash their command alone and ride no blobs.
    /// Counts generate through `summary` against a previous manifest.
    ///
    /// # Arguments
    ///
    /// * `documents` - desired documents in pipeline order, unique per destination.
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
    /// use confit_core::handles::{Route, RouteBase};
    /// use confit_core::plan::Bundle;
    ///
    /// let document = ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
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
        documents.sort_by_key(|document| document.destination.display());
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
    /// use confit_core::handles::{Route, RouteBase};
    /// use confit_core::plan::Bundle;
    ///
    /// let mut previous = Bundle::empty();
    /// previous.manifest.documents = vec![ManifestDocument::new(
    ///     Route::new(RouteBase::Home, "note").unwrap(),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// )];
    /// let bundle = Bundle::build(
    ///     vec![ManifestDocument::new(
    ///         Route::new(RouteBase::Home, "note").unwrap(),
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
}
