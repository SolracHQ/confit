//! Document
//! Desired state payloads reaching disk.

pub mod rc;
pub mod tree;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::routes::Route;
use confit_driver::sha::Sha;
use confit_store::handles::BlobRef;

pub use rc::{RcData, RcEntry, RcOp, RcSection};
pub(crate) use tree::tree_manifest_bytes;
pub use tree::{ManifestMember, Summary, summary, tree_changed};

/// Default unix permission bits for files without explicit modes.
pub const DEFAULT_FILE_MODE: u32 = 0o644;

/// Serialization format for structured documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
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
    /// Return the lowercase format name.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }

    /// Parse a format name in any letter case.
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
    /// Return the lowercase kind name.
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

/// Persisted document payload with binary bytes as refs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Data {
    /// Structured data and its serialization format.
    Structured {
        /// The serialization format.
        format: StructuredFormat,
        /// Payload for rendering.
        data: serde_json::Value,
    },
    /// Plain text content.
    Text {
        /// The exact file text.
        content: String,
        /// Unix permission bits.
        #[serde(default = "default_file_mode")]
        mode: u32,
        /// Presence-only satisfaction flag.
        #[serde(default)]
        unmanaged: bool,
    },
    /// Symlink placement.
    Link {
        /// The link target route.
        target: Route,
    },
    /// The rc data object.
    Rc(RcData),
    /// One blob handle and its mode.
    Opaque {
        /// The content-addressed file bytes identity.
        blob: BlobRef,
        /// Raw byte count of the file content.
        size: u64,
        /// Unix permission bits.
        #[serde(default = "default_file_mode")]
        mode: u32,
        /// Presence-only satisfaction flag.
        #[serde(default)]
        unmanaged: bool,
    },
    /// One managed file set under a destination folder.
    Tree {
        /// Tree members in build order.
        members: Vec<ManifestMember>,
    },
}

impl Data {
    /// Return the kind label for this payload.
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

    /// Return the unix permission bits for this payload.
    pub fn mode(&self) -> Option<u32> {
        match self {
            Self::Text { mode, .. } | Self::Opaque { mode, .. } => Some(*mode),
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => None,
        }
    }

    /// Return the unmanaged flag for this payload.
    pub fn unmanaged(&self) -> bool {
        match self {
            Self::Text { unmanaged, .. } | Self::Opaque { unmanaged, .. } => *unmanaged,
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => false,
        }
    }

    /// Return the tree members for this payload.
    pub fn tree_members(&self) -> Option<&[ManifestMember]> {
        match self {
            Self::Tree { members } => Some(members),
            _ => None,
        }
    }

    /// Collect every blob ref in document order.
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

/// One persisted document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    /// The late-bound destination route.
    pub destination: Route,
    /// The persisted payload.
    pub data: Data,
    /// The hex SHA-256 over rendered bytes.
    pub data_hash: String,
}

/// Per-document lifecycle status against previous documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentStatus {
    /// Document absent from previous documents.
    Create,
    /// Document present with a differing data hash.
    Update,
    /// Document present with an equal data hash.
    Unchanged,
}

impl Document {
    /// Build a document with an empty data hash.
    pub fn new(destination: Route, data: Data) -> Self {
        Self {
            destination,
            data,
            data_hash: String::new(),
        }
    }

    /// Return the kind label for this document.
    pub fn kind(&self) -> DocumentKind {
        self.data.kind()
    }

    /// Return the unix permission bits for this document.
    pub fn mode(&self) -> Option<u32> {
        self.data.mode()
    }

    /// Build one kind with display key.
    pub fn key(&self) -> String {
        format!("{}:{}", self.data.kind().name(), self.destination.display())
    }

    /// Report whether the document carries opaque bytes.
    pub fn is_opaque(&self) -> bool {
        matches!(self.kind(), DocumentKind::Opaque)
    }
}

impl Document {
    /// Report the lifecycle status against previous documents.
    pub fn status(&self, previous: &[Document]) -> DocumentStatus {
        let recorded = previous
            .iter()
            .find(|document| document.key() == self.key());
        match recorded {
            None => {
                let opaque = previous.iter().find(|recorded| {
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

    /// Fill the data hash by rendering the document.
    ///
    /// - The hash covers rendered bytes only.
    /// - Opaque hashes copy the blob ref.
    /// - Tree hashes cover canonical manifest bytes over blob refs.
    ///
    /// # Errors
    ///
    /// - [`Error::Render`] for serializer failures.
    pub fn fill_hash(&mut self) -> Result<()> {
        match &self.data {
            Data::Opaque { blob, .. } => {
                self.data_hash = blob.sha256.hex();
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

    /// Report whether a recorded document yields to desired documents.
    ///
    /// The flag is true while an opaque same-destination sibling exists in desired.
    pub fn superseded_by(&self, desired: &[Document]) -> bool {
        desired.iter().any(|document| {
            document.destination == self.destination
                && document.key() != self.key()
                && (document.is_opaque() || self.is_opaque())
        })
    }
}

/// Render mode bits as octal digits.
pub fn render_mode(mode: u32) -> String {
    format!("{mode:o}")
}

/// Return the default unix permission bits for decode.
fn default_file_mode() -> u32 {
    DEFAULT_FILE_MODE
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
            mode: 0o644,
            unmanaged: false,
        });
        assert_eq!(
            hash, "f007767c80150f15e9cacf96dc8d24ac3b6b3094c9b727d1534b175910968f2a",
            "text hash differs only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_pins_structured_bytes() {
        let data = serde_json::json!({"key": "value"});
        let hash = fill(Data::Structured {
            format: StructuredFormat::Json,
            data,
        });
        assert_eq!(
            hash, "796a0bdfc73f373f33ec3098a246b3d27a10d75e9f4f3dd4e4630efc0f2d3184",
            "structured hash differs only when rendered bytes change"
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
            text.contains("/pinned/sourced.sh"),
            "rc inline bytes carry the display alias: {text}"
        );
        assert_eq!(
            fill(data),
            "4e0bb0e9341fc39990d9a6f3476e68a3d2c49e75102ccb2daa7ba7bfcf275f97",
            "rc hash differs only when rendered bytes change"
        );
    }

    #[test]
    fn fill_hash_copies_opaque_blob_sha() {
        let blob = BlobRef::new(
            confit_driver::sha::Sha::new("aa".repeat(32)).unwrap(),
            confit_driver::sha::Sha::new("bb".repeat(32)).unwrap(),
        );
        let expected = blob.sha256.hex();
        let hash = fill(Data::Opaque {
            blob,
            size: 2,
            mode: 0o644,
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
            confit_driver::sha::Sha::new("aa".repeat(32)).unwrap(),
            confit_driver::sha::Sha::new("bb".repeat(32)).unwrap(),
        );
        let second = BlobRef::new(
            confit_driver::sha::Sha::new("cc".repeat(32)).unwrap(),
            confit_driver::sha::Sha::new("dd".repeat(32)).unwrap(),
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
        let expected =
            confit_driver::sha::Sha::hash(&crate::document::tree_manifest_bytes(&ordered)).hex();
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
