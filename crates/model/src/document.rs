//! Document
//!
//! Desired state payloads reaching disk.

pub mod rc;
pub mod tree;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::manifest::Manifest;
use crate::routes::Route;
use crate::sha::Sha;

pub use rc::{RcData, RcEntry, RcOp, RcSection};
pub(crate) use tree::tree_manifest_bytes;
pub use tree::{ManifestMember, Summary, summary, tree_changed};

/// JSON shaped data table for structured documents.
///
/// Fixed key order keeps equal tables on one bytes form.
///
pub type Table = BTreeMap<String, serde_json::Value>;

/// Serialization format for structured documents.
///
/// # Examples
///
/// ```rust
/// use confit_model::document::StructuredFormat;
///
/// assert!(matches!(StructuredFormat::parse("toml"), Some(StructuredFormat::Toml)));
/// assert!(matches!(StructuredFormat::parse("nope"), None));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StructuredFormat {
    /// JSON format.
    Json,
    /// TOML format.
    Toml,
    /// YAML format.
    Yaml,
}

impl StructuredFormat {
    /// Reads the lowercase format name.
    ///
    /// # Returns
    ///
    /// The format name for keys and log lines.
    ///
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }

    /// Parses a format name in any letter case.
    ///
    /// # Arguments
    ///
    /// * `name` - the raw format name.
    ///
    /// # Returns
    ///
    /// The format for json, toml, or yaml. Else None.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::StructuredFormat;
    ///
    /// assert!(matches!(StructuredFormat::parse("JSON"), Some(StructuredFormat::Json)));
    /// assert!(matches!(StructuredFormat::parse("nope"), None));
    /// ```
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "json" => Some(Self::Json),
            "toml" => Some(Self::Toml),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }
}

impl std::fmt::Display for StructuredFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Document materialization kind.
///
/// Serializes lowercase, like `text`.
///
/// # Examples
///
/// ```rust
/// use confit_model::document::DocumentKind;
///
/// assert!(matches!(DocumentKind::Text.name(), "text"));
/// assert!(matches!(DocumentKind::Opaque.name(), "opaque"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentKind {
    /// Structured data file with a serialization format.
    Structured,
    /// Plain text file from declaration content.
    Text,
    /// Symlink placement from declaration target.
    Link,
    /// Per-shell rc data object.
    Rc,
    /// Raw binary file from declaration bytes.
    Opaque,
    /// Managed file set from one archive under one folder.
    Tree,
}

impl DocumentKind {
    /// Reads the lowercase kind name.
    ///
    /// # Returns
    ///
    /// The kind name for plan keys.
    ///
    pub fn name(&self) -> &'static str {
        match self {
            Self::Structured => "structured",
            Self::Text => "text",
            Self::Link => "link",
            Self::Rc => "rc",
            Self::Opaque => "opaque",
            Self::Tree => "tree",
        }
    }
}

impl std::fmt::Display for DocumentKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

/// Content-addressed bytes ref for manifests.
///
/// The content hash names payload bytes and the stored hash names
/// pool bytes. Both hashes hold 64 hex characters. The store
/// resolves refs into handles at the bundle edge, so manifests carry
/// values alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobRef {
    /// Holds the content hash naming payload bytes.
    pub sha256: Sha,
    /// Holds the pool-bytes hash naming gzip bytes.
    pub stored: Sha,
}

impl BlobRef {
    /// Builds one blob ref from sealed content and stored hashes.
    ///
    /// # Arguments
    ///
    /// * `sha256` - the content hash naming payload bytes.
    /// * `stored` - the pool-bytes hash naming gzip bytes.
    ///
    /// # Returns
    ///
    /// The ref carrying both hashes.
    ///
    pub fn new(sha256: Sha, stored: Sha) -> Self {
        Self { sha256, stored }
    }

    /// Reads the content hash.
    ///
    pub fn sha(&self) -> &Sha {
        &self.sha256
    }

    /// Reads the pool-bytes hash.
    ///
    pub fn stored(&self) -> &Sha {
        &self.stored
    }
}

/// Persisted document payload with binary bytes as refs.
///
/// Serializes externally tagged, like `{ "text": { "content": ".." } }`.
/// Text, structured, rc, and link payloads stay inline.
/// Opaque and tree payloads hold blob refs alone.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Data {
    /// Holds structured data and its serialization format.
    Structured {
        /// Holds the serialization format.
        format: StructuredFormat,
        /// Holds the structured data table.
        data: Table,
    },
    /// Holds plain text content.
    Text {
        /// Holds the exact file text.
        content: String,
        /// Holds unix permission bits. None applies the umask default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
        /// Holds true while presence alone satisfies the document.
        #[serde(default)]
        unmanaged: bool,
    },
    /// Describes a symlink placement.
    Link {
        /// Holds the link target.
        target: String,
    },
    /// Holds the rc data object.
    Rc(RcData),
    /// Holds one blob handle and its mode.
    ///
    /// The unmanaged flag marks presence-only documents.
    /// Present unmanaged documents stay quiet whatever the
    /// bytes. Missing unmanaged documents read as missing.
    Opaque {
        /// Holds the content-addressed file bytes identity.
        blob: BlobRef,
        /// Holds the raw byte count of the file content.
        size: u64,
        /// Holds unix permission bits. None applies the umask default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
        /// Holds true while presence alone satisfies the document.
        #[serde(default)]
        unmanaged: bool,
    },
    /// Holds one managed file set under a destination folder.
    Tree {
        /// Holds members in destination-relative order.
        members: Vec<ManifestMember>,
    },
}

impl Data {
    /// Reads the kind label for this payload.
    ///
    /// # Returns
    ///
    /// The kind matching the payload variant.
    ///
    pub fn kind(&self) -> DocumentKind {
        match self {
            Self::Structured { .. } => DocumentKind::Structured,
            Self::Text { .. } => DocumentKind::Text,
            Self::Link { .. } => DocumentKind::Link,
            Self::Rc(_) => DocumentKind::Rc,
            Self::Opaque { .. } => DocumentKind::Opaque,
            Self::Tree { .. } => DocumentKind::Tree,
        }
    }

    /// Reads the unix permission bits for this payload.
    ///
    /// Text and opaque payloads carry an optional
    /// mode. Every other payload reads as None.
    ///
    /// # Returns
    ///
    /// The mode bits for text and opaque payloads,
    /// else None.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::Data;
    ///
    /// let data = Data::Text { content: "hi".into(), mode: Some(0o755), unmanaged: false};
    /// assert!(matches!(data.mode(), Some(0o755)));
    /// ```
    pub fn mode(&self) -> Option<u32> {
        match self {
            Self::Text { mode, .. } | Self::Opaque { mode, .. } => *mode,
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => None,
        }
    }

    /// Reads the unmanaged flag for this payload.
    ///
    /// Text and opaque payloads carry the flag. Every
    /// other payload reads as false.
    ///
    /// # Returns
    ///
    /// True while presence alone satisfies the document.
    ///
    pub fn unmanaged(&self) -> bool {
        match self {
            Self::Text { unmanaged, .. } | Self::Opaque { unmanaged, .. } => *unmanaged,
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => false,
        }
    }

    /// Reads the tree members for this payload.
    ///
    /// # Returns
    ///
    /// The member list for tree payloads, else None.
    ///
    pub fn tree_members(&self) -> Option<&[ManifestMember]> {
        match self {
            Self::Tree { members } => Some(members),
            _ => None,
        }
    }

    /// Reads every blob ref in document order.
    pub fn blob_refs(&self) -> Vec<&BlobRef> {
        match self {
            Self::Opaque { blob, .. } => vec![blob],
            Self::Tree { members } => members.iter().map(|member| &member.blob).collect(),
            Self::Structured { .. } | Self::Text { .. } | Self::Link { .. } | Self::Rc(_) => {
                Vec::new()
            }
        }
    }
}

/// One persisted document holding metadata and handles.
///
/// The data hash covers rendered bytes, so plan diffs read
/// trusted hashes without pool access. The destination
/// holds a late-bound route, so bundles apply between
/// users and platforms.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// Holds the late-bound destination route.
    pub destination: Route,
    /// Holds the persisted payload.
    pub data: Data,
    /// Holds the hex SHA-256 over rendered bytes.
    pub data_hash: String,
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

impl Document {
    /// Builds a document with an empty data hash.
    ///
    /// # Arguments
    ///
    /// * `destination` - the late-bound destination route.
    /// * `data` - the document payload.
    ///
    /// # Returns
    ///
    /// The document with an empty data hash.
    ///
    pub fn new(destination: Route, data: Data) -> Self {
        Self {
            destination,
            data,
            data_hash: String::new(),
        }
    }

    /// Reads the kind label for this document.
    ///
    /// # Returns
    ///
    /// The kind matching the document payload.
    pub fn kind(&self) -> DocumentKind {
        self.data.kind()
    }

    /// Reads the unix permission bits for this document.
    ///
    /// Text and opaque payloads carry an optional mode.
    /// Every other payload reads as None.
    ///
    /// # Returns
    ///
    /// The mode bits for text and opaque payloads, else None.
    ///
    pub fn mode(&self) -> Option<u32> {
        self.data.mode()
    }

    /// Builds the kind and route key for state lookups.
    ///
    /// # Returns
    ///
    /// The `kind:base:relative` string identifying the state slot.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{Data, Document};
    /// use confit_model::routes::{Route, RouteBase};
    ///
    /// let stored = Document::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
    ///     Data::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert_eq!(stored.key(), "text:home:x");
    /// ```
    pub fn key(&self) -> String {
        format!("{}:{}", self.data.kind().name(), self.destination.display())
    }

    /// Reports whether the document carries opaque bytes.
    ///
    /// # Returns
    ///
    /// True for the opaque kind only.
    ///
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind(), DocumentKind::Opaque)
    }
}

impl Document {
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
    /// use confit_model::document::{Data, Document};
    /// use confit_model::routes::{Route, RouteBase};
    /// use confit_model::document::DocumentStatus;
    /// use confit_model::manifest::Manifest;
    ///
    /// let mut document = Document::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
    ///     Data::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// let previous = Manifest { version: 7, documents: Vec::new(), hooks: Vec::new() };
    /// assert!(matches!(document.status(&previous), DocumentStatus::Create));
    /// ```
    pub fn status(&self, previous: &Manifest) -> DocumentStatus {
        let recorded = previous
            .documents
            .iter()
            .find(|document| document.key() == self.key());
        match recorded {
            None => {
                let opaque = previous.documents.iter().find(|recorded| {
                    recorded.destination == self.destination
                        && recorded.key() != self.key()
                        && (recorded.is_opaque() || self.is_opaque())
                });
                match opaque {
                    Some(_) => DocumentStatus::Update,
                    None => DocumentStatus::Create,
                }
            }
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
    /// copy the blob ref, since the ref carries the
    /// SHA-256 over raw bytes. Tree hashes cover canonical
    /// manifest bytes over blob refs.
    ///
    /// # Returns
    ///
    /// Unit once the hash fills.
    ///
    /// # Errors
    ///
    /// - [`Error::Render`] for serializer failures.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::{Data, Document};
    /// use confit_model::routes::{Route, RouteBase};
    ///
    /// let mut document = Document::new(
    ///     Route::new(RouteBase::Home, "x").unwrap(),
    ///     Data::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert!(matches!(document.fill_hash(), Ok(())));
    /// assert!(matches!(document.data_hash.is_empty(), false));
    /// ```
    pub fn fill_hash(&mut self) -> Result<()> {
        match &self.data {
            Data::Opaque { blob, .. } => {
                self.data_hash = blob.sha().hex();
                Ok(())
            }
            Data::Tree { members } => {
                self.data_hash = Sha::hash(&crate::document::tree_manifest_bytes(members)).hex();
                Ok(())
            }
            inline => {
                let bytes = crate::render::inline_bytes(inline)?;
                self.data_hash = Sha::hash(&bytes).hex();
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
    pub fn superseded_by(&self, desired: &[Document]) -> bool {
        desired.iter().any(|document| {
            document.destination == self.destination
                && document.key() != self.key()
                && (document.is_opaque() || self.is_opaque())
        })
    }
}

/// Renders mode bits as octal digits for drift lines.
///
/// # Examples
///
/// ```rust
/// use confit_model::document::render_mode;
///
/// assert!(matches!(render_mode(0o755).as_str(), "755"));
/// assert!(matches!(render_mode(0o644).as_str(), "644"));
/// ```
pub fn render_mode(mode: u32) -> String {
    format!("{mode:o}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::{Route, RouteBase};

    fn literal(relative: &str) -> Route {
        Route::new(RouteBase::Literal, relative).unwrap()
    }

    fn fill(data: Data) -> String {
        let mut document = Document::new(literal("pinned/subject"), data);
        document.fill_hash().unwrap();
        document.data_hash
    }

    #[test]
    fn fill_hash_pins_text_bytes() {
        let hash = fill(Data::Text {
            content: "pinned text\n".into(),
            mode: None,
            unmanaged: false,
        });
        assert_eq!(
            hash, "f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a",
            "text hash drifts only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_structured_bytes() {
        let mut table: Table = Table::new();
        table.insert("key".to_string(), serde_json::Value::String("value".into()));
        let hash = fill(Data::Structured {
            format: StructuredFormat::Json,
            data: table,
        });
        assert_eq!(
            hash, "796a0bdfc73f373f33ec3098a246b3d27a10d75e9f4f3dd4e4630efc0f2d3184",
            "structured hash drifts only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_rc_display_alias_bytes() {
        let data = Data::Rc(RcData::new(
            vec![RcEntry {
                op: RcOp::Env {
                    name: "EDITOR".into(),
                    value: "hx".into(),
                },
                when: None,
            }],
            vec![RcEntry {
                op: RcOp::Source {
                    path: Route::new(RouteBase::Literal, "/pinned/sourced.sh").unwrap(),
                },
                when: None,
            }],
            vec![RcEntry {
                op: RcOp::Alias {
                    name: "ll".into(),
                    expansion: "ls -l".into(),
                },
                when: None,
            }],
        ));
        let bytes = crate::render::inline_bytes(&data).unwrap();
        let text = String::from_utf8_lossy(&bytes).into_owned();
        assert!(
            text.contains("literal:/pinned/sourced.sh"),
            "rc inline bytes carry the display alias: {text}"
        );
        assert_eq!(
            fill(data),
            "cebb8dd1bf3d0ada60a47b7c2dc670760f11053893ec7f0b324e453f42e5ddaf",
            "rc hash drifts only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_copies_opaque_blob_sha() {
        let blob = BlobRef::new(
            crate::sha::Sha::new("aa".repeat(32)).unwrap(),
            crate::sha::Sha::new("bb".repeat(32)).unwrap(),
        );
        let expected = blob.sha().hex();
        let hash = fill(Data::Opaque {
            blob,
            size: 2,
            mode: None,
            unmanaged: false,
        });
        assert_eq!(
            hash, expected,
            "opaque hash copies content sha without pool bytes"
        );
    }

    #[test]
    fn fill_hash_sorts_tree_members_for_stable_hash() {
        let first = BlobRef::new(
            crate::sha::Sha::new("aa".repeat(32)).unwrap(),
            crate::sha::Sha::new("bb".repeat(32)).unwrap(),
        );
        let second = BlobRef::new(
            crate::sha::Sha::new("cc".repeat(32)).unwrap(),
            crate::sha::Sha::new("dd".repeat(32)).unwrap(),
        );
        let ordered = vec![
            ManifestMember {
                relative: "a".into(),
                blob: first.clone(),
                mode: 0o644,
            },
            ManifestMember {
                relative: "b".into(),
                blob: second.clone(),
                mode: 0o755,
            },
        ];
        let shuffled = vec![
            ManifestMember {
                relative: "b".into(),
                blob: second,
                mode: 0o755,
            },
            ManifestMember {
                relative: "a".into(),
                blob: first,
                mode: 0o644,
            },
        ];
        let expected = crate::sha::Sha::hash(&crate::document::tree_manifest_bytes(&ordered)).hex();
        assert_eq!(
            fill(Data::Tree {
                members: shuffled.clone()
            }),
            expected,
            "tree hash covers sorted manifest bytes"
        );
        assert_eq!(
            fill(Data::Tree { members: ordered }),
            fill(Data::Tree { members: shuffled }),
            "member order leaves tree hash unchanged"
        );
    }
}
