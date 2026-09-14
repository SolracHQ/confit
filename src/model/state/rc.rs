//! Shell Rc
//!
//! Shell rc data per declared shell.

use serde::{Deserialize, Serialize};

use super::condition::Condition;
use super::level::Level;

/// Defines a path-list operation for profile entries.
///
/// The enum offers prepend and append; tools shape PATH-like variables by prepending or
/// appending. Serializes lowercase (`"prepend"` / `"append"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PathOp {
    /// Inserts the directory before existing entries.
    Prepend,
    /// Inserts the directory after existing entries.
    Append,
}

/// Defines the init execution lane inside the final rc group.
///
/// Render emits `first` entries before unmarked ones and `last` entries
/// after them. Serializes lowercase (`"first"`, `"middle"`, `"last"`);
/// deserialization defaults to middle so stored plans without lane data
/// keep their shape.
///
/// # Examples
///
/// ```rust
/// use confit::model::state::rc::Lane;
///
/// assert!(matches!(Lane::parse("first"), Some(Lane::First)));
/// assert!(matches!(Lane::parse("nope"), None));
/// assert!(matches!(Lane::default(), Lane::Middle));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lane {
    /// Runs before unmarked entries.
    First,
    /// Default lane between first and last.
    #[default]
    Middle,
    /// Runs after unmarked entries.
    Last,
}

impl Lane {
    /// Reports middle status for serialization skipping.
    ///
    /// # Arguments
    ///
    /// * `self` - the lane under inspection.
    ///
    /// # Returns
    ///
    /// True for the default middle lane, false for outer lanes.
    pub fn is_middle(&self) -> bool {
        matches!(self, Self::Middle)
    }

    /// Parses a lane name from Lua opts.
    ///
    /// # Arguments
    ///
    /// * `name` - the raw lane name.
    ///
    /// # Returns
    ///
    /// The lane for `first`, `middle`, or `last` (any case), else `None`.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "first" => Some(Self::First),
            "middle" => Some(Self::Middle),
            "last" => Some(Self::Last),
            _ => None,
        }
    }
}

/// Holds one rc entry: a spec plus guard plus priority.///
/// `spec` flattens into the entry JSON, so plan JSON keeps the flat shape:
/// spec fields sit beside `when` plus `priority`.
///
/// # Arguments
///
/// * `spec` - the entry payload, flattened into the entry JSON.
/// * `when` - the optional shell-session guard.
/// * `priority` - the merge priority for the entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry<T> {
    /// Holds the entry payload, flattened into the entry JSON.
    #[serde(flatten)]
    pub spec: T,
    /// Holds the shell-session guard; `None` applies unconditionally.
    pub when: Option<Condition>,
    /// Holds the merge priority.
    pub priority: Level,
}

/// Holds one shell environment variable spec.
///
/// `when = None` marks an unconditional export. Unconditional and conditional entries occupy
/// separate slots. Slots match by name plus structural equality of `when`.
/// First writer wins per slot.
///
/// # Arguments
///
/// * `name` - the variable.
/// * `value` - its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvSpec {
    /// Holds the variable name, e.g. `_ZO_DOCTOR`.
    pub name: String,
    /// Holds the variable value.
    pub value: String,
}

/// Holds one shell environment variable entry.
pub type EnvEntry = Entry<EnvSpec>;

/// Holds one `PATH`-like profile spec.
///
/// Extends [`EnvSpec`] with `op` selecting prepend vs append. Slots
/// match by name plus `when`. First writer wins per slot.
///
/// # Arguments
///
/// * `name` - the variable.
/// * `value` - the directory for prepending or appending.
/// * `op` - selects where the directory goes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileSpec {
    /// Holds the variable name, e.g. `PATH`.
    pub name: String,
    /// Holds the directory for prepending or appending.
    pub value: String,
    /// Selects where the directory goes.
    pub op: PathOp,
}

/// Holds one `PATH`-like profile entry.
pub type ProfileEntry = Entry<ProfileSpec>;

/// Holds one shell alias spec.
///
/// `when = None` marks an unconditional alias. Unconditional and conditional entries occupy
/// separate slots. Slots match by name plus structural equality of `when`.
/// First writer wins per slot.
///
/// # Arguments
///
/// * `name` - the alias.
/// * `value` - its expansion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasSpec {
    /// Holds the alias name, e.g. `cat`.
    pub name: String,
    /// Holds the alias expansion.
    pub value: String,
}

/// Holds one shell alias entry.
pub type AliasEntry = Entry<AliasSpec>;

/// Holds one shell init spec.
///
/// Serializes externally tagged: `{ "eval": { "argv": [...] } }`, `{ "cmd": {
/// "argv": [...] } }`, or `{ "source": { "path": "..." } }`. The Lua
/// `{ eval = {...} }` form maps by wrapping the bare arg array into `argv` at the Lua boundary,
/// while `{ source = "..." }` maps the string into `path`. `lane` rides every
/// variant, defaulting to middle; middle lanes skip serialization so stored
/// plans without lane data keep their shape. `eval` renders `eval
/// "$(argv...)"`; `cmd` renders the argv as a plain command line with
/// shell-escaped args; `source` renders `source path` with shell-escaped path.
/// Entries holding structurally different `when` guards
/// coexist. Identical spec plus guard pairs dedup with first writer winning.
///
/// # Examples
/// ```rust
/// use confit::model::state::level::Level;
/// use confit::model::state::rc::{Entry, InitSpec, Lane};
///
/// let entry = Entry { spec: InitSpec::Eval { argv: vec!["zoxide".into(), "init".into(), "bash".into()], lane: Lane::Middle }, when: None, priority: Level::Normal };
/// assert!(matches!(serde_json::to_value(&entry), Ok(value) if value == serde_json::json!({"eval": {"argv": ["zoxide", "init", "bash"]}, "when": null, "priority": "normal"})));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitSpec {
    /// Evaluates the command output via `eval "$(argv...)"`.
    ///
    /// # Arguments
    ///
    /// * `argv` - the command plus arguments, in order.
    /// * `lane` - the execution lane inside the final group.
    Eval {
        /// Holds the command plus arguments, in order.
        argv: Vec<String>,
        /// Holds the execution lane inside the final group.
        #[serde(default, skip_serializing_if = "Lane::is_middle")]
        lane: Lane,
    },
    /// Runs the command as a plain line.
    ///
    /// # Arguments
    ///
    /// * `argv` - the command plus arguments, in order.
    /// * `lane` - the execution lane inside the final group.
    Cmd {
        /// Holds the command plus arguments, in order.
        argv: Vec<String>,
        /// Holds the execution lane inside the final group.
        #[serde(default, skip_serializing_if = "Lane::is_middle")]
        lane: Lane,
    },
    /// Sources a file into the shell via `source path`.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under sourcing.
    /// * `lane` - the execution lane inside the final group.
    Source {
        /// Holds the file path under sourcing.
        path: String,
        /// Holds the execution lane inside the final group.
        #[serde(default, skip_serializing_if = "Lane::is_middle")]
        lane: Lane,
    },
}

/// Holds one shell init entry.
pub type InitEntry = Entry<InitSpec>;

/// Holds the per-shell rc data object accumulated from config contributions.
///
/// `profile`, `env`, `aliases`, and `init` keep declaration-derived order (config order,
/// then declaration order). An empty value after canonicalization plans zero rc files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RcData {
    /// Holds ordered `PATH`-like entries.
    pub profile: Vec<ProfileEntry>,
    /// Holds ordered environment entries.
    pub env: Vec<EnvEntry>,
    /// Holds ordered alias entries.
    pub aliases: Vec<AliasEntry>,
    /// Holds ordered init entries.
    pub init: Vec<InitEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_entry_keeps_flat_json_shape() {
        let entry = EnvEntry {
            spec: EnvSpec {
                name: "A".into(),
                value: "1".into(),
            },
            when: None,
            priority: Level::Normal,
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({"name": "A", "value": "1", "when": null, "priority": "normal"})
        );
        assert_eq!(
            serde_json::from_value::<EnvEntry>(serde_json::to_value(&entry).unwrap()).unwrap(),
            entry
        );
    }

    #[test]
    fn profile_entry_keeps_flat_json_shape() {
        let entry = ProfileEntry {
            spec: ProfileSpec {
                name: "PATH".into(),
                value: "/a".into(),
                op: PathOp::Prepend,
            },
            when: None,
            priority: Level::High,
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({"name": "PATH", "value": "/a", "op": "prepend", "when": null, "priority": "high"})
        );
        assert_eq!(
            serde_json::from_value::<ProfileEntry>(serde_json::to_value(&entry).unwrap()).unwrap(),
            entry
        );
    }

    #[test]
    fn alias_entry_keeps_flat_json_shape() {
        let entry = AliasEntry {
            spec: AliasSpec {
                name: "cat".into(),
                value: "bat".into(),
            },
            when: Some(Condition::InPath { name: "bat".into() }),
            priority: Level::Low,
        };
        assert_eq!(
            serde_json::to_value(&entry).unwrap(),
            serde_json::json!({"name": "cat", "value": "bat", "when": {"in_path": {"name": "bat"}}, "priority": "low"})
        );
        assert_eq!(
            serde_json::from_value::<AliasEntry>(serde_json::to_value(&entry).unwrap()).unwrap(),
            entry
        );
    }

    #[test]
    fn init_entry_keeps_flat_json_shape() {
        for (spec, tag) in [
            (
                InitSpec::Eval {
                    argv: vec!["zoxide".into(), "init".into()],
                    lane: Lane::Middle,
                },
                "eval",
            ),
            (
                InitSpec::Cmd {
                    argv: vec!["task".into()],
                    lane: Lane::Middle,
                },
                "cmd",
            ),
            (
                InitSpec::Source {
                    path: "~/.cargo/env".into(),
                    lane: Lane::Middle,
                },
                "source",
            ),
        ] {
            let entry = InitEntry {
                spec,
                when: None,
                priority: Level::Major,
            };
            let value = serde_json::to_value(&entry).unwrap();
            assert_eq!(value.get("when"), Some(&serde_json::Value::Null));
            assert_eq!(value.get("priority"), Some(&serde_json::json!("major")));
            assert!(value.get(tag).is_some(), "keeps {tag} tag: {value}");
            assert_eq!(serde_json::from_value::<InitEntry>(value).unwrap(), entry);
        }
    }

    #[test]
    fn lane_parses_names_and_defaults_to_middle() {
        assert_eq!(Lane::parse("first"), Some(Lane::First));
        assert_eq!(Lane::parse("LAST"), Some(Lane::Last));
        assert_eq!(Lane::parse("middle"), Some(Lane::Middle));
        assert_eq!(Lane::parse("nope"), None);
        assert_eq!(Lane::default(), Lane::Middle);
        assert!(Lane::Middle.is_middle());
        assert!(!Lane::First.is_middle());
    }

    #[test]
    fn outer_lanes_serialize_while_middle_skips() {
        let entry = InitEntry {
            spec: InitSpec::Eval {
                argv: vec!["nim".into()],
                lane: Lane::First,
            },
            when: None,
            priority: Level::Normal,
        };
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(
            value
                .get("eval")
                .and_then(|inner| inner.get("lane"))
                .and_then(serde_json::Value::as_str),
            Some("first")
        );
        assert_eq!(serde_json::from_value::<InitEntry>(value).unwrap(), entry);
        let middle = InitEntry {
            spec: InitSpec::Cmd {
                argv: vec!["task".into()],
                lane: Lane::Middle,
            },
            when: None,
            priority: Level::Normal,
        };
        let value = serde_json::to_value(&middle).unwrap();
        assert!(
            value
                .get("cmd")
                .and_then(|inner| inner.get("lane"))
                .is_none(),
            "middle lane skips serialization: {value}"
        );
        let legacy =
            serde_json::json!({"cmd": {"argv": ["task"]}, "when": null, "priority": "normal"});
        assert_eq!(serde_json::from_value::<InitEntry>(legacy).unwrap(), middle);
    }
}
