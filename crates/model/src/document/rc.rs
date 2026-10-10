//! Rc
//! Shell entry shapes for rc files.

use crate::arg::Arg;
use crate::condition::Condition;
use crate::error::{Error, Result};
use crate::routes::Route;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One rc operation for a shell line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RcOp {
    /// Plain value export.
    Env {
        /// The variable name, like `EDITOR`.
        name: String,
        /// The value under export.
        value: String,
    },
    /// PATH like variable around its current value.
    ///
    /// The directory always prepends to the existing entries.
    Path {
        /// The variable name, like `PATH`.
        name: String,
        /// The directory route under placement.
        dir: Route,
    },
    /// Interactive alias.
    Alias {
        /// The alias name, like `ll`.
        name: String,
        /// The alias expansion.
        expansion: String,
    },
    /// Command output evaluation through eval.
    Eval {
        /// Eval command.
        argv: Vec<Arg>,
    },
    /// Plain command line.
    Cmd {
        /// Plain command.
        argv: Vec<Arg>,
    },
    /// File source for the shell.
    Source {
        /// File route for sourcing.
        path: Route,
    },
}

/// One rc entry in any section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RcEntry {
    /// Operation for the shell line.
    #[serde(flatten)]
    pub op: RcOp,
    /// The shell session guard.
    pub when: Option<Condition>,
}

impl RcEntry {
    /// Return the collision slot name.
    pub fn slot_name(&self) -> Option<&str> {
        match &self.op {
            RcOp::Env { name, .. } | RcOp::Path { name, .. } | RcOp::Alias { name, .. } => {
                Some(name.as_str())
            }
            RcOp::Eval { .. } | RcOp::Cmd { .. } | RcOp::Source { .. } => None,
        }
    }

    /// Return the collision log label for the entry.
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

/// Accepted rc section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RcSection {
    /// Entries before the guard.
    Profile,
    /// Entries after the guard.
    Config,
    /// Entries last.
    Final,
}

impl RcSection {
    /// Parse one raw section name.
    ///
    /// # Errors
    ///
    /// - [`Error::Parse`] when the name matches no known section.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "profile" => Ok(Self::Profile),
            "config" => Ok(Self::Config),
            "final" => Ok(Self::Final),
            _ => Err(Error::Parse {
                input: name.to_owned(),
                want: "one of 'profile', 'config', 'final'".to_owned(),
            }),
        }
    }
}

/// Rc data.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RcData {
    /// Entries before the guard.
    pub profile: Vec<RcEntry>,
    /// Entries after the guard.
    pub config: Vec<RcEntry>,
    /// Entries last.
    #[serde(rename = "final")]
    pub final_entries: Vec<RcEntry>,
}

impl RcData {
    /// Build rc data from three entry lists.
    pub fn new(profile: Vec<RcEntry>, config: Vec<RcEntry>, final_entries: Vec<RcEntry>) -> Self {
        Self {
            profile,
            config,
            final_entries,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_rc_section_fails_as_parse_error() {
        assert!(matches!(
            RcSection::parse("profile"),
            Ok(RcSection::Profile)
        ));
        assert!(matches!(RcSection::parse("config"), Ok(RcSection::Config)));
        assert!(matches!(RcSection::parse("final"), Ok(RcSection::Final)));
        let error = match RcSection::parse("confg") {
            Ok(_) => panic!("misspelled section passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Parse { .. }));
    }
}
