//! Side-effect seam: plan export formats, persisted state, IO traits.
//!
//! The service layer depends only on [`StateStore`] and [`PlanWriter`];
//! `memory` provides test fakes, `fs` the real filesystem backends.
//! Serialization lives here ([`PlanFormat::serialize`]); plan works on
//! data and `apply` renders bytes (minijinja, shell codegen).

use std::collections::BTreeMap;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{ArtifactData, Plan, Table};

/// Plan export format selected by the `--format` CLI flag.
///
/// Invariants: closed enum; `Json` is the canonical diffable output,
/// `Toml` is export-only (state files are always JSON on disk).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlanFormat {
    /// Canonical JSON plan output.
    Json,
    /// TOML plan export.
    Toml,
}

impl PlanFormat {
    /// Parse a `--format` flag value.
    ///
    /// Accepts `"json"`/`"toml"` case-insensitively; other values produce
    /// `Err` carrying a plain message string (`String`, directly renderable
    /// by clap).
    /// Args: `s` is the raw flag value.
    ///
    /// Example:
    /// ```rust
    /// use confit::store::PlanFormat;
    ///
    /// assert_eq!(PlanFormat::parse("TOML").unwrap(), PlanFormat::Toml);
    /// assert!(PlanFormat::parse("yaml").is_err());
    /// ```
    pub fn parse(s: &str) -> std::result::Result<Self, String> {
        let lower = s.to_ascii_lowercase();
        match lower.as_str() {
            "json" => Ok(Self::Json),
            "toml" => Ok(Self::Toml),
            _ => Err(format!(
                "unknown plan format '{s}': expected 'json' or 'toml'"
            )),
        }
    }

    /// Serialize a plan to its transfer string.
    ///
    /// JSON goes through `serde_json::to_string_pretty`; TOML through
    /// `toml::to_string` (`created_at` stays a plain `String`, keeping
    /// datetimes directly serializable). TOML export rejects JSON null
    /// anywhere in artifact data with [`Error::Store`] naming the artifact.
    /// Args: `plan` is the merged plan to export.
    pub fn serialize(&self, plan: &Plan) -> Result<String> {
        match self {
            Self::Json => Ok(serde_json::to_string_pretty(plan)?),
            Self::Toml => {
                toml_guard(plan)?;
                toml::to_string(plan)
                    .map_err(|e| Error::Store(format!("serialize plan as TOML: {e}")))
            }
        }
    }
}

impl FromStr for PlanFormat {
    /// Parse a `--format` flag value; see [`PlanFormat::parse`].
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Self::parse(s)
    }
}

/// Persisted apply record: last-written hashes per artifact.
///
/// Key format is `"kind:path"` with the lowercase kind name, e.g.
/// `"rc:/home/tester/.bashrc"` or
/// `"toml:/home/tester/.config/mise/config.toml"`.
/// Invariants: keys use the artifact kind's serialized (lowercase) name;
/// keys serialize in fixed order, so equal states share one bytes form.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    /// Last recorded hashes by `"kind:path"` key.
    pub artifacts: BTreeMap<String, StateEntry>,
}

impl State {
    /// Empty state: every artifact awaits its first apply.
    pub fn empty() -> Self {
        Self {
            artifacts: BTreeMap::new(),
        }
    }
}

/// Last-written hashes plus data snapshot for one artifact.
///
/// Yields the hashes identifying desired and materialized state plus the
/// canonical data snapshot diffing reads entry by entry.
/// Guarantees: `data_hash` tracks the desired data (did the plan change?),
/// `output_hash` tracks the materialized bytes (was the file hand-edited?),
/// `data` holds the canonical artifact-data snapshot (`None` for state files
/// written before snapshots exist); absent snapshots diff as empty previous,
/// so every desired entry reports added. Old state files without `data`
/// still parse via the default.
///
/// Example:
/// ```rust
/// use confit::store::StateEntry;
///
/// let entry = StateEntry { data_hash: "abc".into(), output_hash: "def".into(), data: None };
/// assert!(entry.data.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateEntry {
    /// Hex sha256 of the canonical desired data.
    pub data_hash: String,
    /// Hex sha256 of the materialized output bytes.
    pub output_hash: String,
    /// Canonical artifact-data snapshot (`serde_json::to_value` of the
    /// artifact data); `None` while snapshots are unavailable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// Reads the previous apply record.
///
/// An absent file or an unconfigured path resolves to the empty state; a
/// present-but-corrupt file produces `Err`.
pub trait StateStore {
    /// Load the previous state.
    fn load(&self) -> Result<State>;
}

/// Writes an exported plan to a destination.
///
/// Serializes the plan and publishes it at `dest`, creating parent dirs as
/// needed; all writes land at `dest`.
pub trait PlanWriter {
    /// Serialize `plan` in `format` and store it at `dest`.
    ///
    /// Args: `plan` is the merged plan, `dest` the output path,
    /// `format` the export format.
    fn write(&self, plan: &Plan, dest: &std::path::Path, format: PlanFormat) -> Result<()>;
}

/// Reject TOML-unrepresentable (JSON null) artifact data before serializing.
///
/// Returns [`Error::Store`] naming the offending artifact's `"kind:path"` key.
fn toml_guard(plan: &Plan) -> Result<()> {
    for artifact in &plan.artifacts {
        let bad = match &artifact.data {
            ArtifactData::Toml(table) | ArtifactData::Json(table) | ArtifactData::Yaml(table) => {
                table_has_null(table)
            }
            ArtifactData::Template { vars, .. } => table_has_null(vars),
            ArtifactData::File { .. }
            | ArtifactData::Link { .. }
            | ArtifactData::Fetched { .. }
            | ArtifactData::Rc(_) => false,
        };
        if bad {
            return Err(Error::Store(format!(
                "toml export of artifact '{}:{}' holds a TOML-unrepresentable value (JSON null)",
                artifact.kind, artifact.path,
            )));
        }
    }
    Ok(())
}

/// True when any value in the table (recursively) is JSON null.
fn table_has_null(table: &Table) -> bool {
    table.values().any(value_has_null)
}

/// True when the value is, or contains, JSON null.
fn value_has_null(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => true,
        serde_json::Value::Array(items) => items.iter().any(value_has_null),
        serde_json::Value::Object(map) => map.values().any(value_has_null),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Artifact, ArtifactKind, Contribution};

    fn sample_plan() -> Plan {
        Plan {
            version: 1,
            created_at: "2026-09-09T00:00:00Z".into(),
            root: "/home/tester/confit".into(),
            profile: "desktop".into(),
            artifacts: vec![Artifact {
                kind: ArtifactKind::File,
                path: "/home/tester/.bashrc".into(),
                data: ArtifactData::File {
                    content: "export X=1\n".into(),
                },
                contributions: vec![Contribution {
                    tool: "base".into(),
                    order: 0,
                }],
                shadowed: Default::default(),
                blame: Default::default(),
                data_hash: String::new(),
            }],
            hooks: vec!["mise install".into()],
        }
    }

    #[test]
    fn format_parse_accepts_either_case() {
        assert_eq!(PlanFormat::parse("json").unwrap(), PlanFormat::Json);
        assert_eq!(PlanFormat::parse("JSON").unwrap(), PlanFormat::Json);
        assert_eq!(PlanFormat::parse("toml").unwrap(), PlanFormat::Toml);
        assert_eq!(PlanFormat::parse("TOML").unwrap(), PlanFormat::Toml);
        assert!(PlanFormat::parse("yaml").is_err());
    }

    #[test]
    fn format_from_str_delegates_to_parse() {
        assert_eq!("Json".parse::<PlanFormat>().unwrap(), PlanFormat::Json);
        assert!("yaml".parse::<PlanFormat>().is_err());
    }

    #[test]
    fn json_serialize_round_trips() {
        let plan = sample_plan();
        let text = PlanFormat::Json.serialize(&plan).unwrap();
        assert_eq!(serde_json::from_str::<Plan>(&text).unwrap(), plan);
    }

    #[test]
    fn toml_serialize_smoke() {
        let text = PlanFormat::Toml.serialize(&sample_plan()).unwrap();
        let value: toml::Value = toml::from_str(&text).unwrap();
        assert_eq!(value["profile"].as_str().unwrap(), "desktop");
    }

    #[test]
    fn toml_rejects_null_naming_artifact() {
        let mut plan = sample_plan();
        let mut table = Table::new();
        table.insert(
            "nested".into(),
            serde_json::json!({"list": [serde_json::Value::Null]}),
        );
        plan.artifacts[0].data = ArtifactData::Toml(table);
        let err = PlanFormat::Toml.serialize(&plan).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("/home/tester/.bashrc"), "{msg}");
    }
}
