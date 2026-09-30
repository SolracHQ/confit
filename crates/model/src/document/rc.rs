//! Rc
//!
//! Shell entry shapes for rc files.

use crate::arg::Arg;
use crate::condition::Condition;
use crate::error::{Error, Result};
use crate::routes::Route;

use serde::{Deserialize, Serialize};

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
    ///
    /// The directory always prepends to the existing entries.
    Path {
        /// Holds the variable name, like `PATH`.
        name: String,
        /// Holds the directory route under placement.
        dir: Route,
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
        /// Holds the file route under sourcing.
        path: Route,
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
/// use confit_model::document::{RcEntry, RcOp};
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
    /// use confit_model::document::{RcEntry, RcOp};
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
    /// use confit_model::document::{RcEntry, RcOp};
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
    /// # Arguments
    ///
    /// * `name` - the raw section name.
    ///
    /// # Returns
    ///
    /// The section for profile, config, or final.
    ///
    /// # Errors
    ///
    /// - [`Error::Parse`] when the name matches no known section.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_model::document::RcSection;
    ///
    /// assert!(matches!(RcSection::parse("config"), Ok(RcSection::Config)));
    /// assert!(matches!(RcSection::parse("confg"), Err(_)));
    /// ```
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
///
/// Sections mark position and guard. Any entry kind renders
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
