//! Shell Rc
//!
//! Shell rc data per declared shell.

use serde::{Deserialize, Serialize};

use super::condition::Condition;

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

/// Holds one shell environment variable entry.
///
/// `when = None` marks an unconditional export; unconditional and conditional entries occupy
/// separate slots; shadowing compares `name` plus structural equality of `when` (see
/// `crate::services::merge::merge_env`). Same-slot collisions resolve by highest
/// `priority`, ties by the lexicographically smaller config name.
///
/// # Arguments
///
/// * `name` - the variable.
/// * `value` - its value.
/// * `when` - the optional shell-session guard.
/// * `priority` - merge priority for the entry, defaulting to 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    /// Holds the variable name, e.g. `_ZO_DOCTOR`.
    pub name: String,
    /// Holds the variable value.
    pub value: String,
    /// Holds the shell-session guard; `None` exports unconditionally.
    pub when: Option<Condition>,
    /// Holds the entry merge priority.
    pub priority: u32,
}

/// Holds one `PATH`-like profile entry.
///
/// Extends [`EnvEntry`] with `op` selecting prepend vs append. Shadowing
/// follows the same name-plus-`when` rule (see
/// [`crate::services::merge::merge_profile`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileEntry {
    /// Holds the variable name, e.g. `PATH`.
    pub name: String,
    /// Holds the directory for prepending or appending.
    pub value: String,
    /// Selects where the directory goes.
    pub op: PathOp,
    /// Holds the shell-session guard; `None` applies unconditionally.
    pub when: Option<Condition>,
    /// Holds the entry merge priority.
    pub priority: u32,
}

/// Holds one shell alias entry.
///
/// `when = None` marks an unconditional alias; unconditional and conditional entries occupy
/// separate slots; shadowing compares `name` plus structural equality of `when` (see
/// `crate::services::merge::merge_aliases`).
///
/// # Arguments
///
/// * `name` - the alias.
/// * `value` - its expansion.
/// * `when` - the optional shell-session guard.
/// * `priority` - merge priority for the entry, defaulting to 0.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AliasEntry {
    /// Holds the alias name, e.g. `cat`.
    pub name: String,
    /// Holds the alias expansion.
    pub value: String,
    /// Holds the shell-session guard; `None` defines unconditionally.
    pub when: Option<Condition>,
    /// Holds the entry merge priority.
    pub priority: u32,
}

/// Holds one shell init entry.
///
/// Serializes externally tagged: `{ "eval": { "argv": [...], "when": ... } }`, `{ "cmd": {
/// "argv": [...], "when": ... } }`, or `{ "source": { "path": "...", "when": ... } }`. The Lua
/// `{ eval = {...} }` form maps by wrapping the bare arg array into `argv` at the Lua boundary,
/// while `{ source = "..." }` maps the string into `path`. `eval` renders `eval "$(argv...)"`;
/// `cmd` renders the argv as a plain command line with shell-escaped args; `source` renders
/// `source path` with shell-escaped path. Entries holding structurally different `when` guards
/// coexist; identical spec plus guard pairs dedup (see
/// `crate::services::merge::merge_init`).
///
/// # Examples
/// ```rust
/// use confit::model::state::rc::InitEntry;
///
/// let entry = InitEntry::Eval { argv: vec!["zoxide".into(), "init".into(), "bash".into()], when: None, priority: 0 };
/// assert!(matches!(serde_json::to_value(&entry), Ok(value) if value == serde_json::json!({"eval": {"argv": ["zoxide", "init", "bash"], "when": null, "priority": 0}})));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitEntry {
    /// Evaluates the command output via `eval "$(argv...)"`.
    ///
    /// # Arguments
    ///
    /// * `argv` - the command plus arguments, in order.
    /// * `when` - the optional shell-session guard.
    /// * `priority` - merge priority for the entry, defaulting to 0.
    Eval {
        /// Holds the command plus arguments, in order.
        argv: Vec<String>,
        /// Holds the shell-session guard; `None` runs unconditionally.
        when: Option<Condition>,
        /// Holds the entry merge priority.
        priority: u32,
    },
    /// Runs the command as a plain line.
    ///
    /// # Arguments
    ///
    /// * `argv` - the command plus arguments, in order.
    /// * `when` - the optional shell-session guard.
    /// * `priority` - merge priority for the entry, defaulting to 0.
    Cmd {
        /// Holds the command plus arguments, in order.
        argv: Vec<String>,
        /// Holds the shell-session guard; `None` runs unconditionally.
        when: Option<Condition>,
        /// Holds the entry merge priority.
        priority: u32,
    },
    /// Sources a file into the shell via `source path`.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under sourcing.
    /// * `when` - the optional shell-session guard.
    /// * `priority` - merge priority for the entry, defaulting to 0.
    Source {
        /// Holds the file path under sourcing.
        path: String,
        /// Holds the shell-session guard; `None` runs unconditionally.
        when: Option<Condition>,
        /// Holds the entry merge priority.
        priority: u32,
    },
}

impl InitEntry {
    /// Yields the shell-session guard for the entry.
    ///
    /// # Returns
    ///
    /// The guard, holding `None` for unconditional entries.
    pub fn when(&self) -> Option<&Condition> {
        match self {
            Self::Eval { when, .. } | Self::Cmd { when, .. } | Self::Source { when, .. } => {
                when.as_ref()
            }
        }
    }

    /// Yields the merge priority for the entry.
    ///
    /// # Returns
    ///
    /// The entry priority.
    pub fn priority(&self) -> u32 {
        match self {
            Self::Eval { priority, .. }
            | Self::Cmd { priority, .. }
            | Self::Source { priority, .. } => *priority,
        }
    }
}

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
