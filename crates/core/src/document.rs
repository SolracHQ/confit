//! Document
//!
//! Desired state payloads reaching disk.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::ids::DocPath;

/// JSON shaped data table for structured documents.
///
/// Fixed key order keeps equal tables on one bytes form.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::Table;
///
/// let table = Table::new();
/// assert!(matches!(table.len(), 0));
/// ```
pub type Table = BTreeMap<String, serde_json::Value>;

/// Serialization format for structured documents.
///
/// Serializes lowercase, like `toml`.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::StructuredFormat;
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
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::StructuredFormat;
    ///
    /// assert!(matches!(StructuredFormat::Yaml.name(), "yaml"));
    /// ```
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
    /// use confit_core::document::StructuredFormat;
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

/// Shell session predicate held as data.
///
/// The plan emits entries unconditionally. Each fresh shell session
/// evaluates the guard and skips entries lacking their binary or state.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::Condition;
///
/// let first = Condition::InPath { name: "bat".into() };
/// let second = Condition::InPath { name: "bat".into() };
/// assert!(matches!(first == second, true));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    /// Holds equality between a shell variable and a value.
    EnvEq {
        /// Holds the variable name.
        key: String,
        /// Holds the expected value.
        value: String,
    },
    /// Holds a set, non-empty variable assertion.
    EnvSet {
        /// Holds the variable name.
        key: String,
    },
    /// Holds a binary on PATH assertion.
    InPath {
        /// Holds the binary name.
        name: String,
    },
    /// Holds a path existence assertion.
    Exists {
        /// Holds the path under test.
        path: String,
    },
    /// Holds a conjunction of nested conditions.
    ///
    /// Element order feeds equality.
    All(Vec<Condition>),
    /// Holds a disjunction of nested conditions.
    ///
    /// Element order feeds equality.
    Any(Vec<Condition>),
    /// Holds a negated nested condition.
    ///
    /// Serializes as `nop`.
    #[serde(rename = "nop")]
    Not(Box<Condition>),
}

/// Path list placement for setup entries.
///
/// Prepend leads with the directory.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::PathOp;
///
/// assert!(matches!(PathOp::Prepend, PathOp::Prepend));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PathOp {
    /// Places the directory before existing entries.
    Prepend,
}

/// One rc operation shaping a shell line.
///
/// Env exports a plain value. Path shapes a PATH like variable
/// around its current value. Alias defines an alias.
/// Eval, Cmd, and Source carry execution payloads.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::RcOp;
///
/// let op = RcOp::Alias { name: "ll".into(), expansion: "ls -l".into() };
/// assert!(matches!(op, RcOp::Alias { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RcOp {
    /// Exports a plain value.
    Env {
        /// Holds the variable name, like `EDITOR`.
        name: String,
        /// Holds the value under export.
        value: String,
    },
    /// Shapes a PATH like variable around its current value.
    Path {
        /// Holds the variable name, like `PATH`.
        name: String,
        /// Holds the directory under placement.
        dir: String,
        /// Holds the path placement.
        op: PathOp,
    },
    /// Defines an interactive alias.
    Alias {
        /// Holds the alias name, like `ll`.
        name: String,
        /// Holds the alias expansion.
        expansion: String,
    },
    /// Evaluates command output through eval.
    Eval {
        /// Holds the command plus arguments in order.
        argv: Vec<String>,
    },
    /// Runs a plain command line.
    Cmd {
        /// Holds the command plus arguments in order.
        argv: Vec<String>,
    },
    /// Sources a file into the shell.
    Source {
        /// Holds the file path under sourcing.
        path: String,
    },
}

/// One rc entry in any section.
///
/// The op shapes the shell line. The guard skips the entry in
/// shell sessions lacking its binary or state.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{RcEntry, RcOp};
///
/// let entry = RcEntry {
///     op: RcOp::Env { name: "EDITOR".into(), value: "hx".into() },
///     when: None,
/// };
/// assert!(matches!(entry.slot_name(), Some("EDITOR")));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RcEntry {
    /// Holds the operation shaping the shell line.
    #[serde(flatten)]
    pub op: RcOp,
    /// Holds the shell session guard. None applies unconditionally.
    pub when: Option<Condition>,
}

impl RcEntry {
    /// Reads the collision slot name for the entry.
    ///
    /// # Returns
    ///
    /// The name for env, path, and alias entries. None for exec entries.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{RcEntry, RcOp};
    ///
    /// let entry = RcEntry {
    ///     op: RcOp::Alias { name: "ll".into(), expansion: "ls -l".into() },
    ///     when: None,
    /// };
    /// assert!(matches!(entry.slot_name(), Some("ll")));
    /// ```
    pub fn slot_name(&self) -> Option<&str> {
        match &self.op {
            RcOp::Env { name, .. } | RcOp::Path { name, .. } | RcOp::Alias { name, .. } => {
                Some(name.as_str())
            }
            RcOp::Eval { .. } | RcOp::Cmd { .. } | RcOp::Source { .. } => None,
        }
    }

    /// Reads the collision log label for the entry.
    ///
    /// # Returns
    ///
    /// The lowercase op name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{RcEntry, RcOp};
    ///
    /// let entry = RcEntry {
    ///     op: RcOp::Eval { argv: vec!["mise".into()] },
    ///     when: None,
    /// };
    /// assert!(matches!(entry.log_label(), "eval"));
    /// ```
    pub fn log_label(&self) -> &'static str {
        match &self.op {
            RcOp::Env { .. } => "env",
            RcOp::Path { .. } => "path",
            RcOp::Alias { .. } => "alias",
            RcOp::Eval { .. } => "eval",
            RcOp::Cmd { .. } => "cmd",
            RcOp::Source { .. } => "source",
        }
    }
}

/// Accepted rc section names.
///
/// The engine validates Lua section keys against this list.
/// Misspells fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::RC_SECTION_NAMES;
///
/// assert!(matches!(RC_SECTION_NAMES.contains(&"config"), true));
/// ```
pub const RC_SECTION_NAMES: [&str; 3] = ["profile", "config", "final"];

/// Rc data holding three entry groups.
///
/// Sections mark position plus guard. Any entry kind renders
/// in any section. Profile opens the file. Config holds the
/// interactive block. Final closes the file.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::RcData;
///
/// let data = RcData::new(Vec::new(), Vec::new(), Vec::new());
/// assert!(matches!(data.profile.len(), 0));
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RcData {
    /// Holds entries rendering before the guard.
    pub profile: Vec<RcEntry>,
    /// Holds entries rendering after the guard.
    pub config: Vec<RcEntry>,
    /// Holds entries rendering last.
    #[serde(rename = "final")]
    pub final_entries: Vec<RcEntry>,
}

impl RcData {
    /// Builds rc data from three entry lists.
    ///
    /// # Arguments
    ///
    /// * `profile` - entries rendering before the guard.
    /// * `config` - entries rendering after the guard.
    /// * `final_entries` - entries rendering last.
    ///
    /// # Returns
    ///
    /// The rc data object.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::RcData;
    ///
    /// let data = RcData::new(Vec::new(), Vec::new(), Vec::new());
    /// assert!(matches!(data.config.len(), 0));
    /// ```
    pub fn new(profile: Vec<RcEntry>, config: Vec<RcEntry>, final_entries: Vec<RcEntry>) -> Self {
        Self {
            profile,
            config,
            final_entries,
        }
    }

    /// Validates one rc section name.
    ///
    /// # Arguments
    ///
    /// * `name` - the raw section name.
    ///
    /// # Returns
    ///
    /// Unit for profile, config, or final.
    ///
    /// # Errors
    ///
    /// Unknown names fail as plan errors naming the name.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::RcData;
    ///
    /// assert!(matches!(RcData::check_section_name("config"), Ok(())));
    /// assert!(matches!(RcData::check_section_name("confg"), Err(_)));
    /// ```
    pub fn check_section_name(name: &str) -> Result<()> {
        if RC_SECTION_NAMES.contains(&name) {
            Ok(())
        } else {
            Err(Error::Plan(format!(
                "unknown rc section '{name}': expected {}",
                RC_SECTION_NAMES.join(", ")
            )))
        }
    }
}

/// Document materialization kind.
///
/// Serializes lowercase, like `text`.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::DocumentKind;
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
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::DocumentKind;
    ///
    /// assert!(matches!(DocumentKind::Rc.name(), "rc"));
    /// ```
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

/// Document payload.
///
/// Serializes externally tagged, like `{ "text": { "content": ".." } }`.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::DocumentData;
///
/// let data = DocumentData::Text { content: "hi".into(), mode: None };
/// assert!(matches!(data, DocumentData::Text { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentData {
    /// Holds structured data plus its serialization format.
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
    },
    /// Describes a symlink placement.
    Link {
        /// Holds the link target.
        target: String,
    },
    /// Holds the rc data object.
    Rc(RcData),
    /// Holds raw binary content.
    Opaque {
        /// Holds raw file bytes, base64 in plan JSON.
        #[serde(with = "base64_content")]
        content: Vec<u8>,
        /// Holds unix permission bits. None applies the umask default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
    },
    /// Holds one managed file set under a destination folder.
    Tree {
        /// Holds members in destination-relative order.
        members: Vec<TreeMember>,
    },
}

/// One managed file inside a tree document.
///
/// The relative path lands under the tree destination.
/// Modes always carry explicit bits inherited from the
/// archive member, so trees never depend on the umask.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::TreeMember;
///
/// let member = TreeMember { rel: "font.ttf".into(), content: vec![0x41], mode: 0o644 };
/// assert!(matches!(member.rel.as_str(), "font.ttf"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeMember {
    /// Holds the destination-relative member path.
    pub rel: String,
    /// Holds raw member bytes, base64 in plan JSON.
    #[serde(with = "base64_content")]
    pub content: Vec<u8>,
    /// Holds unix permission bits for the member file.
    pub mode: u32,
}

impl DocumentData {
    /// Reads the kind label for this payload.
    ///
    /// # Returns
    ///
    /// The kind matching the payload variant.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{DocumentData, DocumentKind};
    ///
    /// let data = DocumentData::Text { content: "hi".into(), mode: None };
    /// assert!(matches!(data.kind(), DocumentKind::Text));
    /// ```
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
    /// Text plus opaque payloads carry an optional mode.
    /// Every other payload reads as None.
    ///
    /// # Returns
    ///
    /// The mode bits for text plus opaque payloads, else None.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::DocumentData;
    ///
    /// let data = DocumentData::Text { content: "hi".into(), mode: Some(0o755) };
    /// assert!(matches!(data.mode(), Some(0o755)));
    /// ```
    pub fn mode(&self) -> Option<u32> {
        match self {
            Self::Text { mode, .. } | Self::Opaque { mode, .. } => *mode,
            Self::Structured { .. } | Self::Link { .. } | Self::Rc(_) | Self::Tree { .. } => None,
        }
    }

    /// Reads the tree members for this payload.
    ///
    /// # Returns
    ///
    /// The member list for tree payloads, else None.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::DocumentData;
    ///
    /// let data = DocumentData::Tree { members: Vec::new() };
    /// assert!(matches!(data.tree_members(), Some(_)));
    /// ```
    pub fn tree_members(&self) -> Option<&[TreeMember]> {
        match self {
            Self::Tree { members } => Some(members),
            _ => None,
        }
    }

    /// Builds the persisted payload holding blob references.
    ///
    /// Text, structured, rc, plus link payloads stay inline.
    /// Opaque bytes plus tree member bytes become SHA-256
    /// blob references into the shared pool.
    ///
    /// # Returns
    ///
    /// The manifest payload for plan files plus bundles.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{DocumentData, ManifestData};
    ///
    /// let data = DocumentData::Text { content: "hi".into(), mode: None };
    /// assert!(matches!(data.manifest_data(), ManifestData::Text { .. }));
    /// ```
    pub fn manifest_data(&self) -> ManifestData {
        match self {
            Self::Structured { format, data } => ManifestData::Structured {
                format: *format,
                data: data.clone(),
            },
            Self::Text { content, mode } => ManifestData::Text {
                content: content.clone(),
                mode: *mode,
            },
            Self::Link { target } => ManifestData::Link {
                target: target.clone(),
            },
            Self::Rc(data) => ManifestData::Rc(data.clone()),
            Self::Opaque { content, mode } => ManifestData::Opaque {
                blob: crate::plan::sha256_hex(content),
                mode: *mode,
            },
            Self::Tree { members } => ManifestData::Tree {
                members: members
                    .iter()
                    .map(|member| ManifestMember {
                        rel: member.rel.clone(),
                        blob: crate::plan::sha256_hex(&member.content),
                        mode: member.mode,
                    })
                    .collect(),
            },
        }
    }
}

/// One persisted tree member holding a blob reference.
///
/// The blob names gzipped member bytes under their SHA-256
/// hex in the shared pool. The mode stays inline beside the
/// reference, so manifests read without pool access.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::ManifestMember;
///
/// let member = ManifestMember { rel: "font.ttf".into(), blob: "abc".into(), mode: 0o644 };
/// assert!(matches!(member.rel.as_str(), "font.ttf"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestMember {
    /// Holds the destination-relative member path.
    pub rel: String,
    /// Holds the SHA-256 hex over raw member bytes.
    pub blob: String,
    /// Holds unix permission bits for the member file.
    pub mode: u32,
}

/// Persisted document payload with binary bytes as references.
///
/// Serializes externally tagged, like `{ "text": { "content": ".." } }`.
/// Text, structured, rc, plus link payloads stay inline.
/// Opaque plus tree payloads hold pool blob references alone.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::ManifestData;
///
/// let data = ManifestData::Text { content: "hi".into(), mode: None };
/// assert!(matches!(data, ManifestData::Text { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestData {
    /// Holds structured data plus its serialization format.
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
    },
    /// Describes a symlink placement.
    Link {
        /// Holds the link target.
        target: String,
    },
    /// Holds the rc data object.
    Rc(RcData),
    /// Holds one pool blob reference plus its mode.
    Opaque {
        /// Holds the SHA-256 hex over raw file bytes.
        blob: String,
        /// Holds unix permission bits. None applies the umask default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mode: Option<u32>,
    },
    /// Holds one managed file set under a destination folder.
    Tree {
        /// Holds members in destination-relative order.
        members: Vec<ManifestMember>,
    },
}

impl ManifestData {
    /// Reads every referenced blob hash in document order.
    ///
    /// # Returns
    ///
    /// The blob hashes for opaque plus tree payloads, else empty.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::ManifestData;
    ///
    /// let data = ManifestData::Text { content: "hi".into(), mode: None };
    /// assert!(matches!(data.blob_refs().is_empty(), true));
    /// ```
    pub fn blob_refs(&self) -> Vec<&str> {
        match self {
            Self::Opaque { blob, .. } => vec![blob.as_str()],
            Self::Tree { members } => members.iter().map(|member| member.blob.as_str()).collect(),
            Self::Structured { .. } | Self::Text { .. } | Self::Link { .. } | Self::Rc(_) => {
                Vec::new()
            }
        }
    }
}

/// One persisted document holding metadata plus references.
///
/// The data hash covers rendered bytes exactly like live
/// documents, so plan diffs read trusted hashes without
/// pool access.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData, ManifestDocument};
/// use confit_core::ids::DocPath;
///
/// let document = Document::new(
///     DocPath::new("x"),
///     DocumentData::Text { content: "hi".into(), mode: None },
/// );
/// let stored = document.manifest_document();
/// assert!(matches!(stored.path.as_str(), "x"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDocument {
    /// Holds the destination path.
    pub path: DocPath,
    /// Holds the persisted payload.
    pub data: ManifestData,
    /// Holds the hex SHA-256 over rendered bytes.
    pub data_hash: String,
}

/// Counts changed members between two tree manifests.
///
/// Added plus removed plus content-or-mode modified
/// members count. Order never counts, manifests sort
/// by relative path before comparing.
///
/// # Arguments
///
/// * `old` - the recorded members under comparing.
/// * `new` - the desired members under comparing.
///
/// # Returns
///
/// The changed member count.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{TreeMember, tree_changed};
///
/// let old = vec![TreeMember { rel: "a".into(), content: vec![1], mode: 0o644 }];
/// let new = vec![
///     TreeMember { rel: "a".into(), content: vec![2], mode: 0o644 },
///     TreeMember { rel: "b".into(), content: vec![3], mode: 0o644 },
/// ];
/// assert!(matches!(tree_changed(&old, &new), 2));
/// ```
pub fn tree_changed(old: &[TreeMember], new: &[TreeMember]) -> usize {
    use std::collections::BTreeMap;
    let old_map: BTreeMap<&str, &TreeMember> = old
        .iter()
        .map(|member| (member.rel.as_str(), member))
        .collect();
    let new_map: BTreeMap<&str, &TreeMember> = new
        .iter()
        .map(|member| (member.rel.as_str(), member))
        .collect();
    let mut changed = 0;
    for (rel, member) in &new_map {
        match old_map.get(rel) {
            Some(previous)
                if previous.content == member.content && previous.mode == member.mode =>
            {
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
///
/// Members sort by relative path, so declaration order
/// never leaks into plan hashes. Each line holds the
/// octal mode, the relative path, plus the member sha.
///
/// # Arguments
///
/// * `members` - the tree members under encoding.
///
/// # Returns
///
/// The canonical manifest bytes.
pub(crate) fn tree_manifest_bytes(members: &[TreeMember]) -> Vec<u8> {
    let mut sorted: Vec<&TreeMember> = members.iter().collect();
    sorted.sort_by(|left, right| left.rel.cmp(&right.rel));
    let mut out = Vec::new();
    for member in sorted {
        out.extend_from_slice(
            format!(
                "{:o} {} {}\n",
                member.mode,
                member.rel,
                crate::plan::sha256_hex(&member.content)
            )
            .as_bytes(),
        );
    }
    out
}

/// Base64 string form for opaque bytes in plan JSON.
mod base64_content {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes raw bytes as one base64 string.
    pub(super) fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    /// Deserializes one base64 string into raw bytes.
    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        STANDARD
            .decode(text.as_bytes())
            .map_err(serde::de::Error::custom)
    }
}

/// Parses unix permission bits from octal or symbolic text.
///
/// Octal text holds three digits like `755` or four digits
/// with a leading zero like `0755`. Symbolic text holds nine
/// characters like `rwxr-xr-x`, one `rwx` triple per class.
///
/// # Arguments
///
/// * `text` - the raw mode text.
///
/// # Returns
///
/// The mode bits.
///
/// # Errors
///
/// Leading `d` plus wrong lengths plus bad characters fail
/// as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::parse_mode;
///
/// assert!(matches!(parse_mode("755"), Ok(mode) if mode == 0o755));
/// assert!(matches!(parse_mode("0755"), Ok(mode) if mode == 0o755));
/// assert!(matches!(parse_mode("rwxr-xr-x"), Ok(mode) if mode == 0o755));
/// assert!(matches!(parse_mode("rw-r--r--"), Ok(mode) if mode == 0o644));
/// assert!(matches!(parse_mode("drwxr-xr-x"), Err(_)));
/// ```
pub fn parse_mode(text: &str) -> Result<u32> {
    if text.starts_with('d') {
        return Err(Error::Plan(format!(
            "invalid mode '{text}': leading 'd' marks a directory listing, want octal like 755 or symbolic like rwxr-xr-x"
        )));
    }
    match text.len() {
        3 | 4 => parse_octal_mode(text),
        9 => parse_symbolic_mode(text),
        _ => Err(Error::Plan(format!(
            "invalid mode '{text}': want octal like 755 or symbolic like rwxr-xr-x"
        ))),
    }
}

/// Parses three octal digits with an optional leading zero.
fn parse_octal_mode(text: &str) -> Result<u32> {
    let body = match text.len() {
        3 => text,
        4 => match text.strip_prefix('0') {
            Some(rest) => rest,
            None => {
                return Err(Error::Plan(format!(
                    "invalid mode '{text}': four digit octal starts with 0 like 0755"
                )));
            }
        },
        _ => {
            return Err(Error::Plan(format!(
                "invalid mode '{text}': want octal like 755 or symbolic like rwxr-xr-x"
            )));
        }
    };
    if !body.bytes().all(|byte| matches!(byte, b'0'..=b'7')) {
        return Err(Error::Plan(format!(
            "invalid mode '{text}': octal holds digits 0-7"
        )));
    }
    u32::from_str_radix(body, 8)
        .map_err(|error| Error::Plan(format!("invalid mode '{text}': {error}")))
}

/// Parses nine symbolic characters into mode bits.
fn parse_symbolic_mode(text: &str) -> Result<u32> {
    let bytes = text.as_bytes();
    if bytes.len() != 9 {
        return Err(Error::Plan(format!(
            "invalid mode '{text}': symbolic holds nine rwx characters like rwxr-xr-x"
        )));
    }
    let mut mode: u32 = 0;
    for (index, byte) in bytes.iter().enumerate() {
        let bit: u32 = match (index % 3, byte) {
            (0, b'r') => 4,
            (1, b'w') => 2,
            (2, b'x') => 1,
            (_, b'-') => 0,
            _ => {
                return Err(Error::Plan(format!(
                    "invalid mode '{text}': symbolic holds nine rwx characters like rwxr-xr-x"
                )));
            }
        };
        let shift = (2 - index / 3) * 3;
        mode |= bit << shift;
    }
    Ok(mode)
}

/// Renders mode bits as octal digits for drift lines.
///
/// # Arguments
///
/// * `mode` - the unix mode bits.
///
/// # Returns
///
/// The octal text like `755`.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::render_mode;
///
/// assert!(matches!(render_mode(0o755).as_str(), "755"));
/// assert!(matches!(render_mode(0o644).as_str(), "644"));
/// ```
pub fn render_mode(mode: u32) -> String {
    format!("{mode:o}")
}

/// One materialization step: a path plus its payload.
///
/// The data hash fills during plan builds from rendered bytes.
/// Fresh documents carry an empty hash until the build fills it.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::ids::DocPath;
///
/// let document = Document::new(
///     DocPath::new("x"),
///     DocumentData::Text { content: "hi".into(), mode: None },
/// );
/// assert!(matches!(document.data, DocumentData::Text { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Holds the destination path.
    pub path: DocPath,
    /// Holds the document payload.
    pub data: DocumentData,
    /// Holds the hex SHA-256 over rendered bytes.
    pub data_hash: String,
}

impl Document {
    /// Builds a document with an empty data hash.
    ///
    /// # Arguments
    ///
    /// * `path` - the destination path.
    /// * `data` - the document payload.
    ///
    /// # Returns
    ///
    /// The document with an empty data hash.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("x"),
    ///     DocumentData::Link { target: "dest".into() },
    /// );
    /// assert!(matches!(document.data, DocumentData::Link { .. }));
    /// ```
    pub fn new(path: DocPath, data: DocumentData) -> Self {
        Self {
            path,
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
    /// Text plus opaque payloads carry an optional mode.
    /// Every other payload reads as None.
    ///
    /// # Returns
    ///
    /// The mode bits for text plus opaque payloads, else None.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("x"),
    ///     DocumentData::Text { content: "hi".into(), mode: None },
    /// );
    /// assert!(matches!(document.mode(), None));
    /// ```
    pub fn mode(&self) -> Option<u32> {
        self.data.mode()
    }

    /// Builds the kind plus path key for state lookups.
    ///
    /// # Returns
    ///
    /// The `kind:path` string identifying the state slot.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("x"),
    ///     DocumentData::Text { content: "hi".into(), mode: None },
    /// );
    /// assert!(matches!(document, document if document.key() == "text:x"));
    /// ```
    pub fn key(&self) -> String {
        format!("{}:{}", self.data.kind().name(), self.path.as_str())
    }

    /// Builds the persisted document holding blob references.
    ///
    /// Binary bytes stay live here and move to the pool on
    /// manifest writes. The data hash carries over intact.
    ///
    /// # Returns
    ///
    /// The manifest document for plan files plus bundles.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::document::{Document, DocumentData};
    /// use confit_core::ids::DocPath;
    ///
    /// let document = Document::new(
    ///     DocPath::new("bin"),
    ///     DocumentData::Opaque { content: vec![0xFF], mode: None },
    /// );
    /// assert!(matches!(document.manifest_document().data_hash.as_str(), ""));
    /// ```
    pub fn manifest_document(&self) -> ManifestDocument {
        ManifestDocument {
            path: self.path.clone(),
            data: self.data.manifest_data(),
            data_hash: self.data_hash.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_rc_section_fails_as_plan_error() {
        assert!(matches!(RcData::check_section_name("profile"), Ok(())));
        assert!(matches!(RcData::check_section_name("config"), Ok(())));
        assert!(matches!(RcData::check_section_name("final"), Ok(())));
        let error = match RcData::check_section_name("confg") {
            Ok(()) => panic!("misspelled section passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Plan(_)));
        assert_eq!(
            error.to_string(),
            "unknown rc section 'confg': expected profile, config, final"
        );
    }
}
