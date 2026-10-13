//! Rc
//! Shell entry shapes for rc files.

use confit_binding_macros::Face;

use crate::arg::Arg;
use crate::condition::Condition;
use crate::error::{Error, Result};
use crate::routes::Route;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One rc entry in any section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Face, JsonSchema)]
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
    ///
    /// The directory always prepends to the existing entries.
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
    /// Return the collision slot name.
    pub fn slot_name(&self) -> Option<&str> {
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

/// Accepted rc section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
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
    #[serde(rename = "final")]
    #[face(rename = "final", default)]
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
    use mlua::{Lua, Value};

    use crate::document::Data;
    use crate::routes::RouteBase;
    use confit_binding::value::{Decode, Encode};

    fn test_lua() -> Lua {
        Lua::new()
    }

    fn body_of(lua: &Lua, body: &str) -> Value {
        let source = format!("return {body}");
        lua.load(&source).call(()).unwrap()
    }

    fn documents_of(lua: &Lua, body: &str) -> (Route, Data) {
        let route = Route::new(RouteBase::Home, "subject").unwrap();
        let value = body_of(lua, body);
        match Data::decode(&value, lua) {
            Ok(decoded) => (route, decoded),
            Err(error) => panic!("body decodes: {error}"),
        }
    }

    fn body_error(lua: &Lua, body: &str) -> String {
        let value = body_of(lua, body);
        match Data::decode(&value, lua) {
            Ok(_) => panic!("foreign body passes: {body}"),
            Err(error) => error.to_string(),
        }
    }

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

    #[test]
    fn rc_decodes_every_op() {
        let lua = test_lua();
        let body = r#"{
            profile = {
                { env = { name = "EDITOR", value = "hx", when = { env_eq = { key = "TERM", value = "xterm" } } } },
                { path = { name = "PATH", dir = { base = "home", relative = "bin" } } },
            },
            config = {
                { alias = { name = "ll", expansion = "ls -l" } },
                { eval = { argv = { "mise", "activate" } } },
            },
            final = {
                { cmd = { argv = { "echo", "hi" } } },
                { source = { path = { base = "home", relative = "extra.sh" } } },
            },
        }"#;
        let documents = documents_of(&lua, body);
        match documents.1 {
            Data::Rc(found) => {
                let profile = &found.profile;
                let config = &found.config;
                let final_entries = &found.final_entries;
                assert_eq!(profile.len(), 2);
                assert_eq!(config.len(), 2);
                assert_eq!(final_entries.len(), 2);
                match &profile[0] {
                    RcEntry::Env { name, value, .. } => {
                        assert_eq!(name, "EDITOR");
                        assert_eq!(value, "hx");
                    }
                    _ => panic!("profile holds the env entry"),
                }
                assert!(profile[0].when().is_some(), "guards stay on the entry");
                match &profile[1] {
                    RcEntry::Path { name, dir, .. } => {
                        assert_eq!(name, "PATH");
                        assert_eq!(dir.display(), "home('bin')");
                    }
                    _ => panic!("profile holds the path entry"),
                }
                match &config[0] {
                    RcEntry::Alias {
                        name, expansion, ..
                    } => {
                        assert_eq!(name, "ll");
                        assert_eq!(expansion, "ls -l");
                    }
                    _ => panic!("config holds the alias entry"),
                }
                match &config[1] {
                    RcEntry::Eval { argv, .. } => assert_eq!(argv.len(), 2),
                    _ => panic!("config holds the eval entry"),
                }
                match &final_entries[0] {
                    RcEntry::Cmd { argv, .. } => assert_eq!(argv.len(), 2),
                    _ => panic!("final holds the cmd entry"),
                }
                match &final_entries[1] {
                    RcEntry::Source { path, .. } => {
                        assert_eq!(path.display(), "home('extra.sh')");
                    }
                    _ => panic!("final holds the source entry"),
                }
            }
            _ => panic!("rc body decodes"),
        }
    }

    #[test]
    fn rc_unknown_ops_fail_showing_section() {
        let lua = test_lua();
        for (body, section, fault) in [
            (
                r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
                "RcData.profile",
                "unknown tag 'frobnicate'",
            ),
            (
                r#"{ profile = { { env = { name = "A", value = "B", bogus = 1 } } } }"#,
                "entry 1",
                "unknown field 'bogus'",
            ),
            (
                r#"{ profile = { { env = { name = "A" } } } }"#,
                "entry 1",
                "RcEntry.env",
            ),
        ] {
            let text = body_error(&lua, body);
            assert!(text.contains(section), "the text shows the path: {text}");
            assert!(text.contains(fault), "the text shows the fault: {text}");
        }
    }

    #[test]
    fn rc_live_views_keep_entry_order_through_encode() {
        use crate::arg::Arg;
        use crate::condition::Condition;
        use crate::routes::RouteBase;

        let lua = test_lua();
        let source = RcData::new(
            vec![
                RcEntry::Env {
                    name: "EDITOR".to_owned(),
                    value: "hx".to_owned(),
                    when: Some(Condition::EnvEq {
                        key: "TERM".to_owned(),
                        value: "xterm".to_owned(),
                    }),
                },
                RcEntry::Path {
                    name: "PATH".to_owned(),
                    dir: Route::new(RouteBase::Home, "bin").unwrap(),
                    when: None,
                },
            ],
            vec![RcEntry::Eval {
                argv: vec![
                    Arg::Text("mise".to_owned()),
                    Arg::Route(Route::new(RouteBase::Home, "bin").unwrap()),
                ],
                when: None,
            }],
            vec![RcEntry::Source {
                path: Route::new(RouteBase::Home, "extra.sh").unwrap(),
                when: None,
            }],
        );
        let encoded = source.encode(&lua).unwrap();
        let back = RcData::decode(&encoded, &lua).unwrap();
        assert_eq!(back, source);
    }
}
