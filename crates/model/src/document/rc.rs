//! Rc
//! Shell entry shapes for rc files.

use crate::arg::Arg;
use crate::condition::Condition;
use crate::error::{Error, Result};
use crate::routes::Route;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One rc operation shaping a shell line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RcOp {
    /// Exports a plain value.
    Env {
        /// The variable name, like `EDITOR`.
        name: String,
        /// The value under export.
        value: String,
    },
    /// Shapes a PATH like variable around its current value.
    ///
    /// The directory always prepends to the existing entries.
    Path {
        /// The variable name, like `PATH`.
        name: String,
        /// The directory route under placement.
        dir: Route,
    },
    /// Defines an interactive alias.
    Alias {
        /// The alias name, like `ll`.
        name: String,
        /// The alias expansion.
        expansion: String,
    },
    /// Evaluates command output through eval.
    Eval {
        /// Holds the command and arguments in order.
        argv: Vec<Arg>,
    },
    /// Runs a plain command line.
    Cmd {
        /// Holds the command and arguments in order.
        argv: Vec<Arg>,
    },
    /// Sources a file into the shell.
    Source {
        /// The file route under sourcing.
        path: Route,
    },
}

/// One rc entry in any section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RcEntry {
    /// The operation shaping the shell line.
    #[serde(flatten)]
    pub op: RcOp,
    /// The shell session guard.
    pub when: Option<Condition>,
}

impl RcEntry {
    /// Reads the collision slot name.
    pub fn slot_name(&self) -> Option<&str> {
        match &self.op {
            RcOp::Env { name, .. } | RcOp::Path { name, .. } | RcOp::Alias { name, .. } => {
                Some(name.as_str())
            }
            RcOp::Eval { .. } | RcOp::Cmd { .. } | RcOp::Source { .. } => None,
        }
    }

    /// Reads the collision log label for the entry.
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
    /// Entries rendering before the guard.
    Profile,
    /// Entries rendering after the guard.
    Config,
    /// Entries rendering last.
    Final,
}

impl RcSection {
    /// Parses one raw section name.
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

/// Rc data holding three entry groups.
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
