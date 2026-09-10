//! Defines artifacts, the single type reaching disk.
//!
//! Uses `(kind, path)` as the merge key. Matching path with differing
//! kind produces a plan error. Order-resolved merge losers live in
//! `_shadowed` for `explain`; hashes derive from winners alone.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::rc::{EnvEntry, InitEntry, ProfileEntry, RcData};

/// Defines the JSON-compatible data table for `toml`/`json`/`yaml`
/// artifacts.
///
/// Invariants: serializes with fixed key order, so equal tables share
/// one bytes form. Values survive a JSON round-trip as plain data;
/// `plan` rejects functions and userdata at the Lua boundary. Merge
/// rejects `serde_json::Value::Null` for TOML, naming the path.
pub type Table = BTreeMap<String, serde_json::Value>;

/// Defines the artifact materialization kind.
///
/// Invariants: the closed enum pairs with path as `(kind, path)` merge
/// key, so a kind mismatch on one path produces a merge error.
/// Serializes lowercase (`"toml"`, `"fetched"`, ...). Derives `Hash` for
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
    /// Remote archive unpacked into a directory; last wins.
    Fetched,
    /// Per-shell rc data object; merged per the rc rules.
    Rc,
}

impl fmt::Display for ArtifactKind {
    /// Produces the human name of the kind, matching its serialized form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Toml => "toml",
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Template => "template",
            Self::File => "file",
            Self::Link => "link",
            Self::Fetched => "fetched",
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
    /// Args: `src` is inline content or a project-root-relative path,
    /// `vars` are the render variables.
    Template {
        /// Holds inline content or a project-root-relative path.
        src: String,
        /// Holds the render variables.
        vars: Table,
    },
    /// Holds literal file bytes.
    ///
    /// Args: `content` is the exact file text.
    File {
        /// Holds the exact file text.
        content: String,
    },
    /// Describes a symlink placement.
    ///
    /// Args: `target` is the link target.
    Link {
        /// Holds the link target.
        target: String,
    },
    /// Describes a remote archive unpacked into a directory.
    ///
    /// Args: `source` is the URL, `sha256` pins it, `unpack` is
    /// `zip` | `tar.gz` | `raw`, `include` optionally filters paths.
    Fetched {
        /// Holds the archive URL.
        source: String,
        /// Holds the expected hex sha256 of the archive.
        sha256: String,
        /// Holds the archive layout: `zip` | `tar.gz` | `raw`.
        unpack: String,
        /// Holds the optional path filter inside the archive.
        include: Vec<String>,
    },
    /// Holds the per-shell rc data object.
    Rc(RcData),
}

/// Holds one merge loser for `explain`.
///
/// Yields the losing entry plus the winner attribution for the slot.
/// Guarantees: stays outside hashing and diffing as informational content;
/// `by_tool` and `winner` both name the winning (overlay) tool; `reason`
/// holds a fixed merge-rule string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shadowed<T> {
    /// Names the tool whose entry won the slot.
    pub by_tool: String,
    /// Holds the fixed merge-rule reason, e.g. `"same name, structurally
    /// equal when"`.
    pub reason: String,
    /// Holds the losing entry.
    pub entry: T,
    /// Names the tool whose entry shadowed this one.
    pub winner: String,
}

/// Holds one alias overwrite loser for `explain`.
///
/// Yields the overwritten alias name plus the prior value and winner
/// attribution. Guarantees: one record exists exactly when an overlay key
/// overwrites a base key with a different value; same-value overwrites
/// leave no record and only update blame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasShadow {
    /// Holds the overwritten alias name.
    pub name: String,
    /// Holds the prior (base) alias value.
    pub old_value: String,
    /// Names the tool whose entry won the slot.
    pub by_tool: String,
    /// Names the tool whose entry shadowed this one.
    pub winner: String,
}

/// Holds every merge loser attached to an artifact.
///
/// Yields the debug section backing `_shadowed` in plan output.
/// Guarantees: serves as a debug section; loser-only edits keep winner
/// identity, so the hasher skips the whole set (see `crate::canonical`);
/// `aliases` holds one entry per differing-value overwrite, with
/// same-value overwrites leaving no record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShadowedSet {
    /// Holds shadowed env entries.
    pub env: Vec<Shadowed<EnvEntry>>,
    /// Holds shadowed profile entries.
    pub profile: Vec<Shadowed<ProfileEntry>>,
    /// Holds deduped init entries.
    pub init: Vec<Shadowed<InitEntry>>,
    /// Holds alias overwrites with differing values.
    pub aliases: Vec<AliasShadow>,
}

impl ShadowedSet {
    /// Reports whether every section holds zero shadowed entries.
    ///
    /// Serves as `skip_serializing_if` for `Artifact::shadowed`, letting
    /// plans omit the `_shadowed` key while it stays empty.
    pub fn is_empty(&self) -> bool {
        self.env.is_empty()
            && self.profile.is_empty()
            && self.init.is_empty()
            && self.aliases.is_empty()
    }
}

/// Holds per-entry winner attribution for an artifact.
///
/// Yields the tool owning every winner: maps name exact entry ownership.
/// Guarantees: `env`, `profile`, and `init` align 1:1 with the winners
/// order, built atomically in the same merge pass and serialized together
/// so the vectors cannot diverge; `aliases` maps each alias key to its
/// winner tool; `toml` maps each dotted leaf path to its winner tool;
/// the whole set stays outside the hash, so blame-only edits keep winner
/// identity.
///
/// Example:
/// ```rust
/// use confit::model::artifact::BlameSet;
///
/// let blame = BlameSet::default();
/// assert!(blame.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameSet {
    /// Maps each alias key to its winner tool.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
    /// Holds one winner tool per env winner, aligned 1:1 with winners order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<String>,
    /// Holds one winner tool per profile winner, aligned 1:1 with winners order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<String>,
    /// Holds one winner tool per init winner, aligned 1:1 with winners order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub init: Vec<String>,
    /// Maps each dotted leaf path to its winner tool.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub toml: BTreeMap<String, String>,
}

impl BlameSet {
    /// Reports whether every section holds zero blame entries.
    ///
    /// Serves as `skip_serializing_if` for `Artifact::blame`, letting
    /// plans omit the `_blame` key while it stays empty.
    pub fn is_empty(&self) -> bool {
        self.aliases.is_empty()
            && self.env.is_empty()
            && self.profile.is_empty()
            && self.init.is_empty()
            && self.toml.is_empty()
    }
}

/// Holds blame for one contribution toward an artifact.
///
/// Invariants: `order` comes from the service (profile tool order, then
/// declaration order); the model carries it through merges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Contribution {
    /// Names the contributing tool.
    pub tool: String,
    /// Holds the sequence number assigned by the service.
    pub order: u64,
}

/// Holds one materialization step: a path plus its merged data.
///
/// Yields the canonical merged payload plus debug attributions.
/// Guarantees: `(kind, path)` serves as the merge key; `shadowed` holds
/// losers for `explain` outside the hash; `blame` holds per-entry winners
/// outside the hash; `data_hash` fills post-canonicalization via
/// `crate::canonical::data_hash` and stays outside serialization; freshly
/// merged artifacts carry an empty `data_hash` until the service
/// recomputes it, keeping stale and fresh hashes distinct.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Holds the materialization kind; half of the merge key.
    pub kind: ArtifactKind,
    /// Holds the destination path (or directory for `fetched`); half of
    /// the merge key.
    pub path: String,
    /// Holds the merged payload.
    pub data: ArtifactData,
    /// Holds the blame chain, concatenated across merges in merge order.
    pub contributions: Vec<Contribution>,
    /// Holds merge losers for `explain`; skipped while empty, kept outside
    /// the hash.
    #[serde(
        rename = "_shadowed",
        default,
        skip_serializing_if = "ShadowedSet::is_empty"
    )]
    pub shadowed: ShadowedSet,
    /// Holds per-entry winner attribution; skipped while empty, kept
    /// outside the hash.
    #[serde(rename = "_blame", default, skip_serializing_if = "BlameSet::is_empty")]
    pub blame: BlameSet,
    /// Holds the hex sha256 of canonical data bytes; filled
    /// post-canonicalization.
    #[serde(skip)]
    pub data_hash: String,
}

impl Artifact {
    /// Produces the merge key of the artifact.
    ///
    /// Returns `(kind, path)`; artifacts group and merge under equal keys,
    /// while equal paths with differing kinds produce a merge error.
    pub fn key(&self) -> (ArtifactKind, String) {
        (self.kind, self.path.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_returns_kind_and_path() {
        let artifact = Artifact {
            kind: ArtifactKind::Toml,
            path: "starship.toml".into(),
            data: ArtifactData::Toml(Table::new()),
            contributions: Vec::new(),
            shadowed: ShadowedSet::default(),
            blame: BlameSet::default(),
            data_hash: String::new(),
        };
        assert_eq!(artifact.key(), (ArtifactKind::Toml, "starship.toml".into()));
    }

    #[test]
    fn shadowed_set_emptiness() {
        assert!(ShadowedSet::default().is_empty());
        assert!(BlameSet::default().is_empty());
        let mut set = ShadowedSet::default();
        set.env.push(Shadowed {
            by_tool: "zoxide".into(),
            reason: "same name, structurally equal when".into(),
            entry: EnvEntry {
                name: "X".into(),
                value: "0".into(),
                when: None,
            },
            winner: "zoxide".into(),
        });
        assert!(!set.is_empty());
        let mut aliases = ShadowedSet::default();
        aliases.aliases.push(AliasShadow {
            name: "ls".into(),
            old_value: "a".into(),
            by_tool: "b-tool".into(),
            winner: "b-tool".into(),
        });
        assert!(!aliases.is_empty());
    }

    #[test]
    fn artifact_omits_empty_shadowed_and_hash() {
        let artifact = Artifact {
            kind: ArtifactKind::File,
            path: "x".into(),
            data: ArtifactData::File {
                content: "hi".into(),
            },
            contributions: Vec::new(),
            shadowed: ShadowedSet::default(),
            blame: BlameSet::default(),
            data_hash: "abc".into(),
        };
        let value = serde_json::to_value(&artifact).unwrap();
        assert!(value.get("_shadowed").is_none());
        assert!(value.get("_blame").is_none());
        assert!(value.get("data_hash").is_none());
    }

    #[test]
    fn artifact_omits_empty_blame_subfields() {
        let mut blame = BlameSet::default();
        blame.env.push("zoxide".into());
        let artifact = Artifact {
            kind: ArtifactKind::File,
            path: "x".into(),
            data: ArtifactData::File {
                content: "hi".into(),
            },
            contributions: Vec::new(),
            shadowed: ShadowedSet::default(),
            blame,
            data_hash: String::new(),
        };
        let value = serde_json::to_value(&artifact).unwrap();
        let rendered = value.get("_blame").expect("blame present");
        assert_eq!(
            rendered,
            &serde_json::json!({"env": ["zoxide"]}),
            "{rendered}"
        );
    }
}
