//! Rc
//! Shell entry shapes for rc files.

use confit_binding_macros::Face;

use crate::arg::Arg;
use crate::condition::Condition;
use crate::routes::Route;

use serde::{Deserialize, Serialize};

/// One rc entry in any section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(rename_all = "snake_case")]
#[face(table, tagged)]
pub enum RcEntry {
    /// Plain value export.
    Env {
        /// The variable name, like `EDITOR`.
        name: String,
        /// The value under export.
        value: String,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// PATH like variable around its current value.
    Path {
        /// The variable name, like `PATH`.
        name: String,
        /// The directory route for placement.
        dir: Route,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// Interactive alias.
    Alias {
        /// The alias name, like `ll`.
        name: String,
        /// The alias expansion.
        expansion: String,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// Command output evaluation through eval.
    Eval {
        /// Eval command.
        argv: Vec<Arg>,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// Plain command line.
    Cmd {
        /// Plain command.
        argv: Vec<Arg>,
        /// The shell session guard.
        when: Option<Condition>,
    },
    /// File source for the shell.
    Source {
        /// File route for sourcing.
        path: Route,
        /// The shell session guard.
        when: Option<Condition>,
    },
}

impl RcEntry {
    /// Return the collision key.
    pub fn collision_key(&self) -> Option<&str> {
        match self {
            Self::Env { name, .. } | Self::Path { name, .. } | Self::Alias { name, .. } => {
                Some(name.as_str())
            }
            Self::Eval { .. } | Self::Cmd { .. } | Self::Source { .. } => None,
        }
    }

    /// Return the collision log label for the entry.
    pub fn log_label(&self) -> &'static str {
        match self {
            Self::Env { .. } => "env",
            Self::Path { .. } => "path",
            Self::Alias { .. } => "alias",
            Self::Eval { .. } => "eval",
            Self::Cmd { .. } => "cmd",
            Self::Source { .. } => "source",
        }
    }

    /// Return the shell session guard.
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

/// Shell RC Document.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Face)]
#[serde(deny_unknown_fields)]
#[face(table)]
pub struct RcData {
    /// Entries before the guard.
    #[face(default)]
    pub profile: Vec<RcEntry>,
    /// Entries after the guard.
    #[face(default)]
    pub config: Vec<RcEntry>,
    /// Entries last.
    #[face(default)]
    pub r#final: Vec<RcEntry>,
}

impl RcData {
    /// Build rc data from three entry lists.
    pub fn new(profile: Vec<RcEntry>, config: Vec<RcEntry>, r#final: Vec<RcEntry>) -> Self {
        Self {
            profile,
            config,
            r#final,
        }
    }
}
