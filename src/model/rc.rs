//! Groups shell rc data: profile entries, env entries, aliases, init entries.
//!
//! Accumulates one `RcData` object per declared shell. Ordered lists keep
//! declaration-derived order; maps serialize with fixed key order, so
//! equal data yields equal bytes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::condition::Condition;

/// Defines a path-list operation for profile entries.
///
/// Invariants: the enum offers prepend and append; tools shape PATH-like
/// variables by prepending or appending. Serializes lowercase (`"prepend"`
/// / `"append"`).
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
/// Invariants: `when = None` marks an unconditional export; unconditional
/// and conditional entries occupy separate slots; shadowing compares
/// `name` plus structural equality of `when` (see
/// `crate::merge::merge_env`).
/// Args: `name` and `value` are the variable and its value, `when` holds
/// the optional shell-session guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvEntry {
    /// Holds the variable name, e.g. `_ZO_DOCTOR`.
    pub name: String,
    /// Holds the variable value.
    pub value: String,
    /// Holds the shell-session guard; `None` exports unconditionally.
    pub when: Option<Condition>,
}

/// Holds one `PATH`-like profile entry.
///
/// Extends [`EnvEntry`] with `op` selecting prepend vs append. Shadowing
/// follows the same name-plus-`when` rule (see
/// `crate::merge::merge_profile`).
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
}

/// Holds one shell init entry.
///
/// Serializes externally tagged: `{ "eval": { "argv": [...] } }` or
/// `{ "cmd": { "argv": [...] } }`. The Lua `{ eval = {...} }` form maps
/// by wrapping the bare arg array into `argv` at the Lua boundary.
/// `eval` renders `eval "$(argv...)"`; `cmd` renders the argv as a plain
/// command line with shell-escaped args.
///
/// Example:
/// ```rust
/// use confit::model::InitEntry;
///
/// let entry = InitEntry::Eval { argv: vec!["zoxide".into(), "init".into(), "bash".into()] };
/// assert_eq!(serde_json::to_value(&entry).unwrap(),
///     serde_json::json!({"eval": {"argv": ["zoxide", "init", "bash"]}}));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InitEntry {
    /// Evaluates the command output via `eval "$(argv...)"`.
    ///
    /// Args: `argv` is the command plus arguments, in order.
    Eval {
        /// Holds the command plus arguments, in order.
        argv: Vec<String>,
    },
    /// Runs the command as a plain line.
    ///
    /// Args: `argv` is the command plus arguments, in order.
    Cmd {
        /// Holds the command plus arguments, in order.
        argv: Vec<String>,
    },
}

/// Holds the per-shell rc data object accumulated from tool contributions.
///
/// Invariants: `profile`, `env`, and `init` keep declaration-derived
/// order (profile tool order, then declaration order); `aliases`
/// serializes with fixed key order, so key insertion orders converge to
/// one bytes form. An empty value after canonicalization plans zero rc
/// files.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RcData {
    /// Holds ordered `PATH`-like entries.
    pub profile: Vec<ProfileEntry>,
    /// Holds ordered environment entries.
    pub env: Vec<EnvEntry>,
    /// Holds the alias map, merged key by key with last writer winning.
    pub aliases: BTreeMap<String, String>,
    /// Holds ordered init entries.
    pub init: Vec<InitEntry>,
}
