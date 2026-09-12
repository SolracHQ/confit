//! Tool
//!
//! Tool userdata plus per-tool contributions for Lua.

use mlua::{AnyUserData, Lua, UserData, UserDataMethods, Value};

use super::LuaArtifact;
use super::confit_table;
use super::mise::{MiseSpec, parse_mise_spec};
use super::{boundary_error, field_error, take_string, take_string_array};

/// One shell init entry shape collected from `tool:init`.
///
/// Exactly one entry per `init` call; `argv` holds the command plus arguments in order. The
/// plan service maps this onto the model `InitEntry` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitEntryShape {
    /// Evaluate the command output: `eval "$(argv...)"`.
    ///
    /// # Arguments
    ///
    /// * `argv` - the command plus arguments, in order.
    Eval(Vec<String>),
    /// Run the command as a plain line.
    ///
    /// # Arguments
    ///
    /// * `argv` - the command plus arguments, in order.
    Cmd(Vec<String>),
    /// Source a file into the shell: `source path`.
    ///
    /// # Arguments
    ///
    /// * `path` - the file path under sourcing.
    Source(String),
}

/// Accumulated per-tool contribution built by the `tool:*` methods.
///
/// Each list keeps declaration order; `tags` holds the empty list from the Lua surface for the
/// plan service to fill later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolContribution {
    /// Tool name from `confit.tool(name, ...)`.
    pub tool: String,
    /// Tag list; the Lua surface produces the empty list for the plan
    /// service to fill.
    pub tags: Vec<String>,
    /// Mise package from `opts.install`, if any.
    pub mise: Option<MiseSpec>,
    /// `(name, value)` alias pairs in declaration order.
    pub aliases: Vec<(String, String)>,
    /// `(name, value)` env pairs in declaration order.
    pub envs: Vec<(String, String)>,
    /// `(name, value)` profile pairs in declaration order.
    pub profile_entries: Vec<(String, String)>,
    /// `profile_path` directories in declaration order.
    pub profile_paths: Vec<String>,
    /// Init entries in declaration order.
    pub inits: Vec<InitEntryShape>,
    /// Appended artifact values in declaration order.
    ///
    /// Each entry comes from a `confit.artifact` constructor via
    /// `tool:append_artifact`; the plan service folds them into the
    /// artifact map keyed by `(kind, path)` with `merge_artifact`.
    pub artifacts: Vec<LuaArtifact>,
}

/// Rust-owned builder behind the `confit.tool` userdata.
///
/// Mutated in place by the `tool:*` methods; profile parsing snapshots it
/// into a [`ToolContribution`] via [`ToolBuilder::contribution`].
#[derive(Debug, Clone)]
pub struct ToolBuilder {
    /// Accumulated contribution.
    contribution: ToolContribution,
}

impl ToolBuilder {
    /// Creates a builder holding an initial contribution.
    ///
    /// # Arguments
    ///
    /// * `name` - tool name.
    /// * `mise` - mise spec for the contribution.
    ///
    /// # Returns
    ///
    /// Builder holding the initial contribution.
    fn new(name: String, mise: Option<MiseSpec>) -> Self {
        Self {
            contribution: ToolContribution {
                tool: name,
                mise,
                ..ToolContribution::default()
            },
        }
    }

    /// Returns the accumulated contribution.
    ///
    /// # Returns
    ///
    /// Accumulated contribution.
    pub fn contribution(&self) -> &ToolContribution {
        &self.contribution
    }

    /// Consumes the builder into its accumulated contribution.
    ///
    /// # Returns
    ///
    /// Accumulated contribution.
    pub fn into_contribution(self) -> ToolContribution {
        self.contribution
    }
}

/// Installs the tool constructor on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the constructor.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    confit.set("tool", lua.create_function(tool_callback)?)?;
    Ok(())
}

/// Builds a tool userdata value for `confit.tool`.
///
/// # Arguments
///
/// * `lua` - Lua state holding the userdata.
/// * `name` - raw tool name value.
/// * `opts` - raw options value.
///
/// # Returns
///
/// Tool userdata value.
///
/// # Errors
///
/// Fails with boundary plus field errors for values of other shapes.
pub(crate) fn tool_callback(lua: &Lua, (name, opts): (Value, Value)) -> mlua::Result<AnyUserData> {
    let tool = match name {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(boundary_error(
                "confit.tool: field 'name' must be a string".to_string(),
            ));
        }
    };
    let mise = match opts {
        Value::Nil => None,
        Value::Table(opts) => match opts.get::<Value>("install")? {
            Value::Nil => None,
            Value::Table(spec) => Some(parse_mise_spec(&spec)?),
            _ => {
                return Err(field_error(
                    &tool,
                    "opts.install",
                    "must be the confit.mise.package table",
                ));
            }
        },
        _ => return Err(field_error(&tool, "opts", "must be a table")),
    };
    lua.create_userdata(ToolBuilder::new(tool, mise))
}

/// Parses one `init` spec value into the contribution.
///
/// # Arguments
///
/// * `tool` - tool name for errors.
/// * `spec` - raw spec value.
/// * `contribution` - contribution receiving the entry.
///
/// # Errors
///
/// Fails with field errors for values of other shapes.
fn parse_init_entry(
    tool: &str,
    spec: Value,
    contribution: &mut ToolContribution,
) -> mlua::Result<()> {
    let table = match spec {
        Value::Table(table) => table,
        _ => {
            return Err(field_error(
                tool,
                "init",
                "must be a table with exactly one of 'eval', 'cmd', or 'source'",
            ));
        }
    };
    let eval: Value = table.get("eval")?;
    let cmd: Value = table.get("cmd")?;
    let source: Value = table.get("source")?;
    match (eval, cmd, source) {
        (Value::Table(argv), Value::Nil, Value::Nil) => {
            let argv = take_string_array(&argv, tool, "init.eval")?;
            contribution.inits.push(InitEntryShape::Eval(argv));
            Ok(())
        }
        (Value::Nil, Value::Table(argv), Value::Nil) => {
            let argv = take_string_array(&argv, tool, "init.cmd")?;
            contribution.inits.push(InitEntryShape::Cmd(argv));
            Ok(())
        }
        (Value::Nil, Value::Nil, Value::String(path)) => {
            contribution
                .inits
                .push(InitEntryShape::Source(path.to_string_lossy()));
            Ok(())
        }
        _ => Err(field_error(
            tool,
            "init",
            "must hold exactly one of 'eval' or 'cmd' with a string array, or 'source' with a string path",
        )),
    }
}

impl UserData for ToolBuilder {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("alias", |_, this, (name, value): (Value, Value)| {
            let tool = this.contribution.tool.clone();
            let name = take_string(name, &tool, "alias")?;
            let value = take_string(value, &tool, "alias")?;
            this.contribution.aliases.push((name, value));
            Ok(())
        });
        methods.add_method_mut("env", |_, this, (name, value): (Value, Value)| {
            let tool = this.contribution.tool.clone();
            let name = take_string(name, &tool, "env")?;
            let value = take_string(value, &tool, "env")?;
            this.contribution.envs.push((name, value));
            Ok(())
        });
        methods.add_method_mut("profile", |_, this, (name, value): (Value, Value)| {
            let tool = this.contribution.tool.clone();
            let name = take_string(name, &tool, "profile")?;
            let value = take_string(value, &tool, "profile")?;
            this.contribution.profile_entries.push((name, value));
            Ok(())
        });
        methods.add_method_mut("profile_path", |_, this, dir: Value| {
            let tool = this.contribution.tool.clone();
            let dir = take_string(dir, &tool, "profile_path")?;
            this.contribution.profile_paths.push(dir);
            Ok(())
        });
        methods.add_method_mut("init", |_, this, spec: Value| {
            let tool = this.contribution.tool.clone();
            parse_init_entry(&tool, spec, &mut this.contribution)
        });
        methods.add_method_mut("append_artifact", |_, this, artifact: Value| {
            let tool = this.contribution.tool.clone();
            match artifact {
                Value::UserData(handle) => match handle.borrow::<LuaArtifact>() {
                    Ok(item) => {
                        this.contribution.artifacts.push(item.clone());
                        Ok(())
                    }
                    Err(_) => Err(field_error(
                        &tool,
                        "append_artifact",
                        "must be a confit.artifact value",
                    )),
                },
                _ => Err(field_error(
                    &tool,
                    "append_artifact",
                    "must be a confit.artifact value",
                )),
            }
        });
    }
}
