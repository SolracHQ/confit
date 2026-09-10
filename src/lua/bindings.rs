//! `confit` global: tool handles and mise package specs.
//!
//! `confit.tool` returns Rust-owned userdata mutated in place, which is why
//! the Lua surface uses method syntax (`tool:alias(...)`); plain
//! constructors and data (`confit.mise.package`) use `.`.

use mlua::{AnyUserData, Lua, Table, UserData, UserDataMethods, Value};

use crate::error::Error;

/// Mise package spec from `confit.mise.package`.
///
/// Invariants: `name` is the package name; `version` is always populated,
/// defaulting to `"latest"` when the Lua table omits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSpec {
    /// Package name, e.g. `"bat"`.
    pub name: String,
    /// Pinned version or `"latest"`.
    pub version: String,
}

/// One shell init entry shape collected from `tool:init`.
///
/// Invariants: exactly one entry per `init` call; `argv` holds the command
/// plus arguments in order. The plan service maps this onto the model
/// `InitEntry` type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitEntryShape {
    /// Evaluate the command output: `eval "$(argv...)"`.
    ///
    /// Args: `argv` is the command plus arguments, in order.
    Eval(Vec<String>),
    /// Run the command as a plain line.
    ///
    /// Args: `argv` is the command plus arguments, in order.
    Cmd(Vec<String>),
}

/// Accumulated per-tool contribution built by the `tool:*` methods.
///
/// Invariants: each list keeps declaration order; `tags` holds the empty
/// list from the Lua surface for the plan service to fill later.
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
    /// Start a contribution for `name` with an optional mise spec.
    ///
    /// Args: `name` is the tool name, `mise` the parsed `opts.install`.
    fn new(name: String, mise: Option<MiseSpec>) -> Self {
        Self {
            contribution: ToolContribution {
                tool: name,
                mise,
                ..ToolContribution::default()
            },
        }
    }

    /// Borrow the accumulated contribution.
    ///
    /// Used by profile parsing to snapshot the builder state.
    pub fn contribution(&self) -> &ToolContribution {
        &self.contribution
    }

    /// Consume the builder into its accumulated contribution.
    pub fn into_contribution(self) -> ToolContribution {
        self.contribution
    }
}

/// Wrap a message as a Lua-domain error for the eval boundary.
///
/// The eval layer unwraps this back into `Error::Lua`, passing the message
/// through unchanged; messages name tool and field.
fn boundary_error(message: String) -> mlua::Error {
    mlua::Error::external(Error::Lua(message))
}

/// Build a field error naming the tool and field.
///
/// Args: `tool` is the tool name, `field` the Lua field, `detail` the
/// expectation that was violated.
fn field_error(tool: &str, field: &str, detail: &str) -> mlua::Error {
    boundary_error(format!("tool '{tool}': field '{field}' {detail}"))
}

/// Read a required string from a Lua value.
///
/// Args: `value` is the raw Lua value, `tool`/`field` name the error.
fn take_string(value: Value, tool: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(field_error(tool, field, "must be a string")),
    }
}

/// Read a dense string array from a Lua table.
///
/// Integer keys cover `1..=len` with string values; the reader rejects
/// functions and nested tables with a field error. Args: `table` is the
/// array, `tool`/`field` name the error.
fn take_string_array(table: &Table, tool: &str, field: &str) -> mlua::Result<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let index = match key {
            Value::Integer(index) => index,
            _ => return Err(field_error(tool, field, "must be a string array")),
        };
        let item = match value {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(field_error(
                    tool,
                    field,
                    &format!("entry [{index}] must be a string"),
                ));
            }
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in indexed.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(field_error(
                tool,
                field,
                "must be a dense string array starting at 1",
            ));
        }
    }
    Ok(indexed.into_iter().map(|(_, item)| item).collect())
}

/// Parse the `confit.mise.package` return table into a [`MiseSpec`].
///
/// `version` defaults to `"latest"`. Args: `table` is the spec table.
fn parse_mise_spec(table: &Table) -> mlua::Result<MiseSpec> {
    let name = match table.get::<Value>("name")? {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(boundary_error(
                "mise.package: field 'name' must be a string".to_string(),
            ));
        }
    };
    let version = match table.get::<Value>("version")? {
        Value::Nil => "latest".to_string(),
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(boundary_error(
                "mise.package: field 'version' must be a string".to_string(),
            ));
        }
    };
    Ok(MiseSpec { name, version })
}

/// Build the `confit.tool(name, opts)` callback.
///
/// `opts.install`, when present, must be the `confit.mise.package` table.
fn tool_callback(lua: &Lua, (name, opts): (Value, Value)) -> mlua::Result<AnyUserData> {
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

/// Build the `confit.mise.package({ name, version? })` callback.
fn mise_package_callback(lua: &Lua, spec: Value) -> mlua::Result<Table> {
    let input = match spec {
        Value::Table(input) => input,
        _ => {
            return Err(boundary_error(
                "mise.package: argument must be a table with a string 'name'".to_string(),
            ));
        }
    };
    let parsed = parse_mise_spec(&input)?;
    let output = lua.create_table()?;
    output.set("name", parsed.name)?;
    output.set("version", parsed.version)?;
    Ok(output)
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
            let table = match spec {
                Value::Table(table) => table,
                _ => {
                    return Err(field_error(
                        &tool,
                        "init",
                        "must be a table with exactly one of 'eval' or 'cmd'",
                    ));
                }
            };
            let eval: Value = table.get("eval")?;
            let cmd: Value = table.get("cmd")?;
            match (eval, cmd) {
                (Value::Table(argv), Value::Nil) => {
                    let argv = take_string_array(&argv, &tool, "init.eval")?;
                    this.contribution.inits.push(InitEntryShape::Eval(argv));
                    Ok(())
                }
                (Value::Nil, Value::Table(argv)) => {
                    let argv = take_string_array(&argv, &tool, "init.cmd")?;
                    this.contribution.inits.push(InitEntryShape::Cmd(argv));
                    Ok(())
                }
                _ => Err(field_error(
                    &tool,
                    "init",
                    "must hold exactly one of 'eval' or 'cmd' with a string array",
                )),
            }
        });
    }
}

/// Install the `confit` global on a fresh Lua state.
///
/// Sets `confit.tool` and `confit.mise.package`.
/// Args: `lua` is the state receiving the global.
pub fn install_confit(lua: &Lua) -> mlua::Result<()> {
    let mise = lua.create_table()?;
    mise.set("package", lua.create_function(mise_package_callback)?)?;
    let confit = lua.create_table()?;
    confit.set("tool", lua.create_function(tool_callback)?)?;
    confit.set("mise", mise)?;
    lua.globals().set("confit", confit)?;
    Ok(())
}
