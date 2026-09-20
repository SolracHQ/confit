//! Document
//!
//! Desired state payloads reaching disk.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::condition::Condition;
use crate::error::{Error, Result};
use crate::ids::DocPath;

/// JSON shaped data table for structured documents.
///
/// Fixed key order keeps equal tables on one bytes form.
///
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

/// Path list placement for setup entries.
///
/// Prepend leads with the directory.
///
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
#[serde(deny_unknown_fields)]
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
pub const RC_SECTION_NAMES: [&str; 3] = ["profile", "config", "final"];

/// Rc data holding three entry groups.
///
/// Sections mark position plus guard. Any entry kind renders
/// in any section. Profile opens the file. Config holds the
/// interactive block. Final closes the file.
///
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

/// One persisted tree member holding a blob reference.
///
/// The blob names gzipped member bytes under their SHA-256
/// hex in the shared pool. The mode stays inline beside the
/// reference, so manifests read without pool access.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestMember {
    /// Holds the destination-relative member path.
    pub relative: String,
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
    /// Holds one pool blob reference plus its mode.
    ///
    /// The unmanaged flag marks presence-only documents.
    /// Present unmanaged documents stay quiet whatever the
    /// bytes. Missing unmanaged documents read as missing.
    Opaque {
        /// Holds the SHA-256 hex over raw file bytes.
        blob: String,
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

impl ManifestData {
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
    /// use confit_core::document::ManifestData;
    ///
    /// let data = ManifestData::Text { content: "hi".into(), mode: Some(0o755), unmanaged: false};
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
    /// Text plus opaque payloads carry the flag. Every
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

    /// Reads every referenced blob hash in document order.
    ///
    /// # Returns
    ///
    /// The blob hashes for opaque plus tree payloads, else empty.
    ///
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
/// The data hash covers rendered bytes, so plan diffs read
/// trusted hashes without pool access.
///
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestDocument {
    /// Holds the destination path.
    pub path: DocPath,
    /// Holds the persisted payload.
    pub data: ManifestData,
    /// Holds the hex SHA-256 over rendered bytes.
    pub data_hash: String,
}

impl ManifestDocument {
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
    pub fn new(path: DocPath, data: ManifestData) -> Self {
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
    /// use confit_core::document::{ManifestData, ManifestDocument};
    /// use confit_core::ids::DocPath;
    ///
    /// let stored = ManifestDocument::new(
    ///     DocPath::new("x"),
    ///     ManifestData::Text { content: "hi".into(), mode: None, unmanaged: false},
    /// );
    /// assert_eq!(stored.key(), "text:x");
    /// ```
    pub fn key(&self) -> String {
        format!("{}:{}", self.data.kind().name(), self.path.as_str())
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
/// use confit_core::document::{ManifestMember, tree_changed};
///
/// let old = vec![ManifestMember { relative: "a".into(), blob: "aa".into(), mode: 0o644 }];
/// let new = vec![
///     ManifestMember { relative: "a".into(), blob: "bb".into(), mode: 0o644 },
///     ManifestMember { relative: "b".into(), blob: "cc".into(), mode: 0o644 },
/// ];
/// assert_eq!(tree_changed(&old, &new), 2);
/// ```
pub fn tree_changed(old: &[ManifestMember], new: &[ManifestMember]) -> usize {
    use std::collections::BTreeMap;
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
///
/// Members sort by relative path, so declaration order
/// never leaks into plan hashes. Each line holds the
/// octal mode, the relative path, plus the member blob hash.
///
/// # Arguments
///
/// * `members` - the tree members under encoding.
///
/// # Returns
///
/// The canonical manifest bytes.
pub(crate) fn tree_manifest_bytes(members: &[ManifestMember]) -> Vec<u8> {
    let mut sorted: Vec<&ManifestMember> = members.iter().collect();
    sorted.sort_by(|left, right| left.relative.cmp(&right.relative));
    let mut out = Vec::new();
    for member in sorted {
        out.extend_from_slice(
            format!("{:o} {} {}\n", member.mode, member.relative, member.blob).as_bytes(),
        );
    }
    out
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
