//! Artifact
//!
//! The single type reaching disk.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::Result;

use super::rc::RcData;

/// Defines the JSON-compatible data table for `toml`/`json`/`yaml` artifacts.
///
/// Serializes with fixed key order, so equal tables share one bytes form. Values survive a JSON
/// round-trip as plain data; `plan` rejects functions and userdata at the Lua boundary. Merge
/// rejects `serde_json::Value::Null` for TOML, naming the path.
pub type Table = BTreeMap<String, serde_json::Value>;

/// Defines the artifact materialization kind.
///
/// The closed enum pairs with path as `(kind, path)` merge key, so a kind mismatch on one path
/// produces a merge error. Serializes lowercase (`"toml"`, `"link"`, ...). Derives `Hash` for
/// merge-key use in maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ArtifactKind {
    /// TOML config file, deep-merged.
    Toml,
    /// JSON config file, deep-merged.
    Json,
    /// YAML config file, deep-merged.
    Yaml,
    /// Minijinja template rendered at apply time; last wins.
    Template,
    /// Literal-bytes file escape hatch; last wins.
    File,
    /// Symlink placement; last wins.
    Link,
    /// Per-shell rc data object; merged per the rc rules.
    Rc,
}

impl fmt::Display for ArtifactKind {
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
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Template => "template",
            Self::File => "file",
            Self::Link => "link",
            Self::Rc => "rc",
        };
        f.write_str(name)
    }
}

/// Holds the artifact payload.
///
/// Serializes externally tagged with snake_case tags: `{ "toml": {...} }`,
/// `{ "template": { "src": "..", "vars": {...} } }`, `{ "rc": {...} }`,
/// and so on. Template `vars` carries the same fixed-key-order guarantee.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactData {
    /// Holds the TOML config table.
    Toml(Table),
    /// Holds the JSON config table.
    Json(Table),
    /// Holds the YAML config table.
    Yaml(Table),
    /// Holds template content plus variables.
    ///
    /// # Arguments
    ///
    /// * `src` - inline content or a project-root-relative path.
    /// * `vars` - the render variables.
    Template {
        /// Holds inline content or a project-root-relative path.
        src: String,
        /// Holds the render variables.
        vars: Table,
    },
    /// Holds literal file bytes.
    ///
    /// # Arguments
    ///
    /// * `content` - the exact file text.
    File {
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

impl ArtifactData {
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
/// from `ArtifactData::to_bytes` plus the `security` hash and stays outside
/// serialization; freshly merged artifacts carry an empty `data_hash` until
/// the service recomputes it, keeping stale and fresh hashes distinct.
///
/// # Returns
///
/// The canonical merged payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Holds the materialization kind; half of the merge key.
    pub kind: ArtifactKind,
    /// Holds the destination path; half of the `(kind, path)` merge key.
    pub path: String,
    /// Holds the merged payload.
    pub data: ArtifactData,
    /// Holds the hex sha256 of canonical data bytes; filled
    /// post-merge.
    #[serde(skip)]
    pub data_hash: String,
}

impl Artifact {
    /// Builds the kind plus path key string for the artifact.
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
        let artifact = Artifact {
            kind: ArtifactKind::Toml,
            path: "starship.toml".into(),
            data: ArtifactData::Toml(Table::new()),
            data_hash: String::new(),
        };
        assert_eq!(artifact.key_string(), "toml:starship.toml");
    }

    #[test]
    fn artifact_omits_hash() {
        let artifact = Artifact {
            kind: ArtifactKind::File,
            path: "x".into(),
            data: ArtifactData::File {
                content: "hi".into(),
            },
            data_hash: "abc".into(),
        };
        let value = serde_json::to_value(&artifact).unwrap();
        assert!(value.get("data_hash").is_none());
        assert!(value.get("_shadowed").is_none());
        assert!(value.get("_blame").is_none());
        assert!(value.get("contributions").is_none());
    }

    #[test]
    fn artifact_round_trips_without_provenance() {
        let artifact = Artifact {
            kind: ArtifactKind::File,
            path: "x".into(),
            data: ArtifactData::File {
                content: "hi".into(),
            },
            data_hash: String::new(),
        };
        let value = serde_json::to_value(&artifact).unwrap();
        assert!(serde_json::from_value::<Artifact>(value).is_ok());
    }
}
