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
/// Prepend leads with the directory. Append trails with it.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::PathOp;
///
/// assert!(matches!(PathOp::Prepend, PathOp::Prepend));
/// assert!(matches!(PathOp::Append == PathOp::Prepend, false));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PathOp {
    /// Places the directory before existing entries.
    Prepend,
    /// Places the directory after existing entries.
    Append,
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
/// let data = DocumentData::Text { content: "hi".into() };
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
    },
    /// Describes a symlink placement.
    Link {
        /// Holds the link target.
        target: String,
    },
    /// Holds the rc data object.
    Rc(RcData),
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
    /// let data = DocumentData::Text { content: "hi".into() };
    /// assert!(matches!(data.kind(), DocumentKind::Text));
    /// ```
    pub fn kind(&self) -> DocumentKind {
        match self {
            Self::Structured { .. } => DocumentKind::Structured,
            Self::Text { .. } => DocumentKind::Text,
            Self::Link { .. } => DocumentKind::Link,
            Self::Rc(_) => DocumentKind::Rc,
        }
    }
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
///     DocumentData::Text { content: "hi".into() },
/// );
/// assert!(matches!(document.data, DocumentData::Text { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    /// Holds the destination path.
    pub path: DocPath,
    /// Holds the document payload.
    pub data: DocumentData,
    /// Holds the hex SHA-256 over rendered bytes. Filled by plan builds.
    #[serde(skip)]
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
    ///     DocumentData::Text { content: "hi".into() },
    /// );
    /// assert!(matches!(document, document if document.key() == "text:x"));
    /// ```
    pub fn key(&self) -> String {
        format!("{}:{}", self.data.kind().name(), self.path.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_document(path: &str, content: &str) -> Document {
        Document::new(
            DocPath::new(path),
            DocumentData::Text {
                content: content.to_string(),
            },
        )
    }

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
            "plan error: unknown rc section 'confg': expected profile, config, final"
        );
    }

    #[test]
    fn document_key_prefixes_kind() {
        assert!(text_document("x", "hi").key() == "text:x");
    }

    #[test]
    fn rc_entry_roundtrips_through_json() {
        let entry = RcEntry {
            op: RcOp::Cmd {
                argv: vec!["task".to_string()],
            },
            when: None,
        };
        let value = match serde_json::to_value(&entry) {
            Ok(value) => value,
            Err(error) => panic!("rc entry serializes: {error}"),
        };
        let parsed = match serde_json::from_value::<RcEntry>(value) {
            Ok(entry) => entry,
            Err(error) => panic!("rc entry parses: {error}"),
        };
        assert_eq!(parsed, entry);
    }

    #[test]
    fn condition_serializes_tagged_shapes() {
        let guarded = Condition::InPath { name: "bat".into() };
        let guarded_value = match serde_json::to_value(&guarded) {
            Ok(value) => value,
            Err(error) => panic!("condition serializes: {error}"),
        };
        assert_eq!(
            guarded_value,
            serde_json::json!({"in_path": {"name": "bat"}})
        );
        let negated = Condition::Not(Box::new(Condition::EnvSet {
            key: "SSH_TTY".into(),
        }));
        let negated_value = match serde_json::to_value(&negated) {
            Ok(value) => value,
            Err(error) => panic!("condition serializes: {error}"),
        };
        assert_eq!(
            negated_value,
            serde_json::json!({"nop": {"env_set": {"key": "SSH_TTY"}}})
        );
        let both = Condition::All(vec![
            Condition::EnvEq {
                key: "TERM_PROGRAM".into(),
                value: "WarpTerminal".into(),
            },
            guarded.clone(),
        ]);
        let round_tripped =
            match serde_json::from_value::<Condition>(match serde_json::to_value(&both) {
                Ok(value) => value,
                Err(error) => panic!("condition serializes: {error}"),
            }) {
                Ok(condition) => condition,
                Err(error) => panic!("condition parses: {error}"),
            };
        assert_eq!(both, round_tripped);
    }

    #[test]
    fn structured_format_names_and_parses() {
        assert_eq!(
            StructuredFormat::parse("json"),
            Some(StructuredFormat::Json)
        );
        assert_eq!(
            StructuredFormat::parse("TOML"),
            Some(StructuredFormat::Toml)
        );
        assert_eq!(
            StructuredFormat::parse("yaml"),
            Some(StructuredFormat::Yaml)
        );
        assert_eq!(StructuredFormat::parse("nope"), None);
        let value = match serde_json::to_value(StructuredFormat::Toml) {
            Ok(value) => value,
            Err(error) => panic!("format serializes: {error}"),
        };
        assert_eq!(value, serde_json::json!("toml"));
    }
}
