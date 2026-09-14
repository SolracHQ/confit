//! Document
//!
//! The single type reaching disk.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::rc::RcData;

/// Defines the JSON-compatible data table for structured documents.
///
/// Serializes with fixed key order, so equal tables share one bytes form. Values survive a JSON
/// round-trip as plain data; `plan` rejects functions and userdata at the Lua boundary. Merge
/// rejects `serde_json::Value::Null` for TOML, naming the path.
pub type Table = BTreeMap<String, serde_json::Value>;

/// Defines the serialization format for structured documents.
///
/// The closed enum pairs with the data table inside
/// [`DocumentData::Structured`], so one merge rule covers every format.
/// Serializes lowercase (`"toml"`, `"json"`, `"yaml"`).
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
    /// Yields the lowercase format name.
    ///
    /// # Returns
    ///
    /// The format name for log lines and snapshot tags.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

impl fmt::Display for StructuredFormat {
    /// Renders the lowercase format name.
    ///
    /// # Arguments
    ///
    /// * `f` - the sink receiving the format name.
    ///
    /// # Errors
    ///
    /// Formatting failures from the sink.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Defines the document materialization kind.
///
/// The closed enum pairs with path as `(kind, path)` merge key, so a kind mismatch on one path
/// produces a merge error. Serializes lowercase (`"structured"`, `"text"`, ...). Derives `Hash`
/// for merge-key use in maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentKind {
    /// Structured data file, merged on the inner data table.
    Structured,
    /// Plain text file, declaration only.
    Text,
    /// Symlink placement; last wins.
    Link,
    /// Per-shell rc data object; merged per the rc rules.
    Rc,
}

impl fmt::Display for DocumentKind {
    /// Renders the lowercase kind name.
    ///
    /// # Arguments
    ///
    /// * `f` - the sink receiving the kind name.
    ///
    /// # Errors
    ///
    /// Formatting failures from the sink.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Structured => "structured",
            Self::Text => "text",
            Self::Link => "link",
            Self::Rc => "rc",
        };
        f.write_str(name)
    }
}

/// Holds the document payload.
///
/// Serializes externally tagged with snake_case tags: `{ "structured": {
/// "format": "toml", "data": {...} } }`, `{ "text": { "content": ".." } }`,
/// `{ "link": { "target": ".." } }`, `{ "rc": {...} }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentData {
    /// Holds structured data plus its serialization format.
    ///
    /// # Arguments
    ///
    /// * `format` - the serialization format.
    /// * `data` - the structured data table.
    Structured {
        /// Holds the serialization format.
        format: StructuredFormat,
        /// Holds the structured data table.
        data: Table,
    },
    /// Holds plain text content.
    ///
    /// # Arguments
    ///
    /// * `content` - the exact file text.
    Text {
        /// Holds the exact file text.
        content: String,
    },
    /// Describes a symlink placement.
    ///
    /// # Arguments
    ///
    /// * `target` - the link target.
    Link {
        /// Holds the link target.
        target: String,
    },
    /// Holds the per-shell rc data object.
    Rc(RcData),
}

impl DocumentData {
    /// Yields canonical JSON bytes for the payload.
    ///
    /// Tables serialize with fixed key order, so equal payloads share one
    /// bytes form.
    ///
    /// # Returns
    ///
    /// Canonical bytes for hashing and comparison.
    ///
    /// # Errors
    ///
    /// Serialization failures surface as JSON errors.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(self)?)
    }
}

/// Holds one materialization step: a path plus its merged data.
///
/// `(Kind, path)` serves as the merge key; `data_hash` fills post-merge
/// from `DocumentData::to_bytes` plus the `security` hash and stays outside
/// serialization; freshly merged documents carry an empty `data_hash` until
/// the service recomputes it, keeping stale and fresh hashes distinct.
///
/// # Returns
///
/// The canonical merged payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Holds the materialization kind; half of the merge key.
    pub kind: DocumentKind,
    /// Holds the destination path; half of the `(kind, path)` merge key.
    pub path: String,
    /// Holds the merged payload.
    pub data: DocumentData,
    /// Holds the hex sha256 of canonical data bytes; filled
    /// post-merge.
    #[serde(skip)]
    pub data_hash: String,
}

impl Document {
    /// Builds the kind plus path key string for the document.
    ///
    /// # Returns
    ///
    /// The kind plus path string identifying the merge slot.
    pub fn key_string(&self) -> String {
        format!("{}:{}", self.kind, self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_string_combines_kind_and_path() {
        let document = Document {
            kind: DocumentKind::Structured,
            path: "starship.toml".into(),
            data: DocumentData::Structured {
                format: StructuredFormat::Toml,
                data: Table::new(),
            },
            data_hash: String::new(),
        };
        assert_eq!(document.key_string(), "structured:starship.toml");
    }

    #[test]
    fn document_omits_hash() {
        let document = Document {
            kind: DocumentKind::Text,
            path: "x".into(),
            data: DocumentData::Text {
                content: "hi".into(),
            },
            data_hash: "abc".into(),
        };
        let value = serde_json::to_value(&document).unwrap();
        assert!(value.get("data_hash").is_none());
        assert!(value.get("_shadowed").is_none());
        assert!(value.get("_blame").is_none());
        assert!(value.get("contributions").is_none());
    }

    #[test]
    fn document_round_trips_without_provenance() {
        let document = Document {
            kind: DocumentKind::Text,
            path: "x".into(),
            data: DocumentData::Text {
                content: "hi".into(),
            },
            data_hash: String::new(),
        };
        let value = serde_json::to_value(&document).unwrap();
        assert!(serde_json::from_value::<Document>(value).is_ok());
    }

    #[test]
    fn structured_format_round_trips_every_format() {
        for format in [
            StructuredFormat::Json,
            StructuredFormat::Toml,
            StructuredFormat::Yaml,
        ] {
            let data = DocumentData::Structured {
                format,
                data: Table::new(),
            };
            let value = serde_json::to_value(&data).unwrap();
            assert_eq!(
                value
                    .get("structured")
                    .and_then(|inner| inner.get("format"))
                    .and_then(serde_json::Value::as_str),
                Some(format.name())
            );
            assert_eq!(serde_json::from_value::<DocumentData>(value).unwrap(), data);
        }
        assert_eq!(
            serde_json::to_value(StructuredFormat::Yaml).unwrap(),
            serde_json::json!("yaml")
        );
    }
}
