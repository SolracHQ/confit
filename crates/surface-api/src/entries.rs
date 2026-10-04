//! Rc entries
//! Rc live tables with section shapes for patch callbacks.

use mlua::{FromLua, Lua, LuaSerdeExt, Table, Value};
use serde::Deserialize;

use confit_model::arg::Arg;
use confit_model::condition::Condition;
use confit_model::document::{RcEntry, RcOp};
use confit_model::routes::Route;

use crate::document::RcDocument;
use crate::error::ApiError;
use crate::paths::route_value;

/// One rc document shape holding three entry lists.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RcDocumentShape {
    /// Entries rendering before the guard.
    #[serde(default)]
    profile: Vec<RcEntryShape>,
    /// Entries rendering after the guard.
    #[serde(default)]
    config: Vec<RcEntryShape>,
    /// Entries rendering last.
    #[serde(default, rename = "final")]
    final_entries: Vec<RcEntryShape>,
}

/// One rc entry shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RcEntryShape {
    /// Exports a plain value.
    #[serde(default)]
    env: Option<EnvShape>,
    /// Shapes a PATH like variable.
    #[serde(default)]
    path: Option<PathShape>,
    /// Defines an interactive alias.
    #[serde(default)]
    alias: Option<AliasShape>,
    /// Evaluates command output through eval.
    #[serde(default)]
    eval: Option<ArgvShape>,
    /// Runs a plain command line.
    #[serde(default)]
    cmd: Option<ArgvShape>,
    /// Sources a file into the shell.
    #[serde(default)]
    source: Option<SourceShape>,
    /// The shell session guard. None applies unconditionally.
    #[serde(default)]
    when: Option<Condition>,
}

/// One env inner shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvShape {
    /// The variable name, like `EDITOR`.
    name: String,
    /// The value under export.
    value: String,
}

/// One path inner shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathShape {
    /// The variable name, like `PATH`.
    name: String,
    /// The directory route under placement.
    dir: Route,
}

/// One alias inner shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AliasShape {
    /// The alias name, like `ll`.
    name: String,
    /// The alias expansion.
    expansion: String,
}

/// One argv inner shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ArgvShape {
    /// Holds the command and arguments in order.
    argv: Vec<Arg>,
}

/// One source inner shape.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceShape {
    /// The file route under sourcing.
    path: Route,
}

impl FromLua for RcDocument {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        let shape: RcDocumentShape = decode_rc_shape(value)?;
        Ok(Self {
            profile: convert_entries(shape.profile, "profile")?,
            config: convert_entries(shape.config, "config")?,
            final_entries: convert_entries(shape.final_entries, "final")?,
        })
    }
}

/// Decodes one rc document shape naming its section.
///
/// # Errors
///
/// - [`ApiError::Mismatch`](crate::error::ApiError::Mismatch) for misshaped
///   sections and entries.
fn decode_rc_shape(value: Value) -> mlua::Result<RcDocumentShape> {
    serde_path_to_error::deserialize(mlua::serde::Deserializer::new(value)).map_err(|error| {
        let detail = error.inner().to_string();
        let field = rc_field(&error.path().to_string());
        ApiError::mismatch(field, detail.clone(), detail).into()
    })
}

/// Returns one rc field for its serde path.
fn rc_field(path: &str) -> String {
    let trimmed = path.strip_prefix('.').unwrap_or(path);
    let section = trimmed.split(['.', '[']).next().unwrap_or("");
    match section {
        "profile" | "config" | "final" => {
            if trimmed.len() == section.len() {
                ["rc ", section, " section"].concat()
            } else {
                entry_field(section)
            }
        }
        _ => {
            if trimmed.is_empty() {
                "body".to_owned()
            } else {
                trimmed.to_owned()
            }
        }
    }
}

/// Converts one entry list into model entries.
///
/// # Errors
///
/// - [`ApiError::Mismatch`](crate::error::ApiError::Mismatch) for doubled
///   and missing ops.
fn convert_entries(list: Vec<RcEntryShape>, section: &str) -> mlua::Result<Vec<RcEntry>> {
    let mut entries = Vec::with_capacity(list.len());
    for entry in list {
        entries.push(entry.into_entry(section)?);
    }
    Ok(entries)
}

impl RcEntryShape {
    /// Converts one entry shape into its model entry.
    ///
    /// # Errors
    ///
    /// - [`ApiError::Mismatch`](crate::error::ApiError::Mismatch) for doubled
    ///   and missing ops.
    fn into_entry(self, section: &str) -> mlua::Result<RcEntry> {
        let present = [
            ("env", self.env.is_some()),
            ("path", self.path.is_some()),
            ("alias", self.alias.is_some()),
            ("eval", self.eval.is_some()),
            ("cmd", self.cmd.is_some()),
            ("source", self.source.is_some()),
        ];
        if present.iter().filter(|(_, has)| *has).count() != 1 {
            let names = present
                .iter()
                .filter(|(_, has)| *has)
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(ApiError::mismatch(
                entry_field(section),
                "one of 'env', 'path', 'alias', 'eval', 'cmd' or 'source'",
                names,
            )
            .into());
        }
        let op = if let Some(inner) = self.env {
            RcOp::Env {
                name: inner.name,
                value: inner.value,
            }
        } else if let Some(inner) = self.path {
            RcOp::Path {
                name: inner.name,
                dir: inner.dir,
            }
        } else if let Some(inner) = self.alias {
            RcOp::Alias {
                name: inner.name,
                expansion: inner.expansion,
            }
        } else if let Some(inner) = self.eval {
            RcOp::Eval { argv: inner.argv }
        } else if let Some(inner) = self.cmd {
            RcOp::Cmd { argv: inner.argv }
        } else if let Some(inner) = self.source {
            RcOp::Source { path: inner.path }
        } else {
            return Err(ApiError::mismatch(
                entry_field(section),
                "one of 'env', 'path', 'alias', 'eval', 'cmd' or 'source'",
                "",
            )
            .into());
        };
        Ok(RcEntry {
            op,
            when: self.when,
        })
    }
}

/// Returns the entry field name for one section.
fn entry_field(section: &str) -> String {
    ["rc ", section, " entry"].concat()
}

impl RcDocument {
    /// Builds the live body table for patch callbacks.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for table build failures.
    pub fn live_table(&self, lua: &Lua) -> mlua::Result<Table> {
        let body = lua.create_table()?;
        body.set("profile", live_entries(lua, &self.profile)?)?;
        body.set("config", live_entries(lua, &self.config)?)?;
        body.set("final", live_entries(lua, &self.final_entries)?)?;
        Ok(body)
    }
}

/// Builds one live section list from entries.
fn live_entries(lua: &Lua, entries: &[RcEntry]) -> mlua::Result<Table> {
    let list = lua.create_table()?;
    for (index, entry) in entries.iter().enumerate() {
        list.set(index + 1, live_entry(lua, entry)?)?;
    }
    Ok(list)
}

/// Builds one live entry table from its model entry.
fn live_entry(lua: &Lua, entry: &RcEntry) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    let inner = lua.create_table()?;
    match &entry.op {
        RcOp::Env { name, value } => {
            inner.set("name", name.as_str())?;
            inner.set("value", value.as_str())?;
            table.set("env", inner)?;
        }
        RcOp::Path { name, dir } => {
            inner.set("name", name.as_str())?;
            inner.set("dir", live_route(lua, dir)?)?;
            table.set("path", inner)?;
        }
        RcOp::Alias { name, expansion } => {
            inner.set("name", name.as_str())?;
            inner.set("expansion", expansion.as_str())?;
            table.set("alias", inner)?;
        }
        RcOp::Eval { argv } => {
            inner.set("argv", live_argv(lua, argv)?)?;
            table.set("eval", inner)?;
        }
        RcOp::Cmd { argv } => {
            inner.set("argv", live_argv(lua, argv)?)?;
            table.set("cmd", inner)?;
        }
        RcOp::Source { path } => {
            inner.set("path", live_route(lua, path)?)?;
            table.set("source", inner)?;
        }
    }
    if let Some(guard) = &entry.when {
        table.set("when", lua.to_value(guard)?)?;
    }
    Ok(table)
}

/// Builds one live argv list from args.
fn live_argv(lua: &Lua, argv: &[Arg]) -> mlua::Result<Table> {
    let list = lua.create_table()?;
    for (index, arg) in argv.iter().enumerate() {
        list.set(index + 1, live_arg(lua, arg)?)?;
    }
    Ok(list)
}

/// Builds one live argv entry from its arg.
fn live_arg(lua: &Lua, arg: &Arg) -> mlua::Result<Value> {
    match arg {
        Arg::Text(text) => Ok(Value::String(lua.create_string(text.as_str())?)),
        Arg::Route(route) => live_route(lua, route),
    }
}

/// Builds one live route table from its route.
fn live_route(lua: &Lua, route: &Route) -> mlua::Result<Value> {
    route_value(lua, route)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::decode_config;
    use crate::document::Document;
    use confit_model::document::RcOp;

    fn test_lua() -> Lua {
        Lua::new()
    }

    fn config_of(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    fn documents_of(lua: &Lua, body: &str) -> (confit_model::routes::Route, Document) {
        let source = [
            "return { name = \"tools/test\", documents = { [{ base = \"home\", relative = \"subject\" }] = ",
            body,
            " } }",
        ]
        .concat();
        let value = config_of(lua, &source);
        match decode_config(lua, &value) {
            Ok(decoded) => {
                assert_eq!(decoded.documents.len(), 1);
                decoded.documents.into_iter().next().unwrap()
            }
            Err(error) => panic!("body decodes: {error}"),
        }
    }

    fn body_error(lua: &Lua, body: &str) -> String {
        let source = [
            "return { name = \"tools/test\", documents = { [{ base = \"home\", relative = \"subject\" }] = ",
            body,
            " } }",
        ]
        .concat();
        let value = config_of(lua, &source);
        match decode_config(lua, &value) {
            Ok(_) => panic!("foreign body passes: {body}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn rc_decodes_every_op() {
        let lua = test_lua();
        let body = r#"{
            profile = {
                { env = { name = "EDITOR", value = "hx" }, when = { env_eq = { key = "TERM", value = "xterm" } } },
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
            Document::Rc(found) => {
                let profile = &found.profile;
                let config = &found.config;
                let final_entries = &found.final_entries;
                assert_eq!(profile.len(), 2);
                assert_eq!(config.len(), 2);
                assert_eq!(final_entries.len(), 2);
                match &profile[0].op {
                    RcOp::Env { name, value } => {
                        assert_eq!(name, "EDITOR");
                        assert_eq!(value, "hx");
                    }
                    _ => panic!("profile holds the env entry"),
                }
                assert!(profile[0].when.is_some(), "guards ride the entry");
                match &profile[1].op {
                    RcOp::Path { name, dir } => {
                        assert_eq!(name, "PATH");
                        assert_eq!(dir.display(), "home('bin')");
                    }
                    _ => panic!("profile holds the path entry"),
                }
                match &config[0].op {
                    RcOp::Alias { name, expansion } => {
                        assert_eq!(name, "ll");
                        assert_eq!(expansion, "ls -l");
                    }
                    _ => panic!("config holds the alias entry"),
                }
                match &config[1].op {
                    RcOp::Eval { argv } => assert_eq!(argv.len(), 2),
                    _ => panic!("config holds the eval entry"),
                }
                match &final_entries[0].op {
                    RcOp::Cmd { argv } => assert_eq!(argv.len(), 2),
                    _ => panic!("final holds the cmd entry"),
                }
                match &final_entries[1].op {
                    RcOp::Source { path } => assert_eq!(path.display(), "home('extra.sh')"),
                    _ => panic!("final holds the source entry"),
                }
            }
            _ => panic!("rc body decodes"),
        }
    }

    #[test]
    fn rc_unknown_ops_fail_naming_entry() {
        let lua = test_lua();
        for (body, want) in [
            (
                r#"{ profile = { { frobnicate = { name = "x" } } } }"#,
                "frobnicate",
            ),
            (
                r#"{ profile = { { env = { name = "A", value = "B" }, bogus = 1 } } }"#,
                "bogus",
            ),
            (r#"{ profile = { { env = { name = "A" } } } }"#, "value"),
        ] {
            let text = body_error(&lua, body);
            assert!(text.contains(want), "names the fault: {text}");
        }
    }
}
