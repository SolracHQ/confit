//! Shell entry shapes for rc files.

use lua_binding::Shape;

use crate::arg::Arg;
use crate::condition::Condition;
use crate::routes::Route;

use serde::{Deserialize, Serialize};

/// One rc entry in any section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(rename_all = "snake_case")]
#[shape(table, typed_alias)]
pub enum RcEntry {
    /// A variable that the shell exports with a value.
    Env {
        /// The variable name, like `EDITOR`.
        name: String,
        /// The exported value.
        value: String,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// A variable that gains one directory on PATH.
    Path {
        /// The variable name, like `PATH`.
        name: String,
        /// The directory that the entry appends.
        dir: Route,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// A shell alias with its expansion.
    Alias {
        /// The alias name, like `ll`.
        name: String,
        /// The alias expansion.
        expansion: String,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// A command whose output the shell evaluates.
    Eval {
        /// The command whose output runs in the shell.
        argv: Vec<Arg>,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// A command that the shell runs as written.
    Cmd {
        /// The command line.
        argv: Vec<Arg>,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// A file the shell sources.
    Source {
        /// The file to source.
        path: Route,
        /// The shell session guard.
        when: Option<Condition>,
    },
}

impl RcEntry {
    /// Returns the name that two entries collide on.
    pub fn collision_key(&self) -> Option<&str> {
        match self {
            Self::Env { name, .. } | Self::Path { name, .. } | Self::Alias { name, .. } => {
                Some(name.as_str())
            }
            Self::Eval { .. } | Self::Cmd { .. } | Self::Source { .. } => None,
        }
    }

    /// Returns the condition that guards the entry.
    pub fn when(&self) -> Option<&Condition> {
        match self {
            Self::Env { when, .. }
            | Self::Path { when, .. }
            | Self::Alias { when, .. }
            | Self::Eval { when, .. }
            | Self::Cmd { when, .. }
            | Self::Source { when, .. } => when.as_ref(),
        }
    }
}

/// Shell entries in three sections.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Shape)]
#[serde(deny_unknown_fields)]
#[shape(table)]
pub struct RcData {
    /// Entries before the guard.
    #[shape(default)]
    pub profile: Vec<RcEntry>,
    /// Entries after the guard.
    #[shape(default)]
    pub config: Vec<RcEntry>,
    /// Entries that close the rc file.
    #[shape(default)]
    pub r#final: Vec<RcEntry>,
}

impl RcData {
    /// Assembles the three sections into an rc document.
    pub fn new(profile: Vec<RcEntry>, config: Vec<RcEntry>, r#final: Vec<RcEntry>) -> Self {
        Self {
            profile,
            config,
            r#final,
        }
    }
}
