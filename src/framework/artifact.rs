//! Artifact
//!
//! Artifact value constructors for Lua. The file kinds build opaque userdata
//! holding merge key plus payload plus priority. The `rc` subtable builds
//! entry handles holding area plus payload plus guard plus priority, tuned
//! through `:when` and `:with_priority` methods. Both submit through
//! `config:add_artifact`, which converts handles into contribution entries
//! for the plan service to fold.

use std::collections::BTreeMap;

use mlua::{Lua, Table, UserData, UserDataMethods, Value};
use serde_json::Value as Json;

use crate::error::Error;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::artifact::Table as ModelTable;
use crate::model::state::condition::Condition;

use super::confit_table;

/// Opaque artifact value built by the `confit.artifact` constructors.
///
/// Carries the merge key (`kind`, `path`) plus the payload (`data`) plus the
/// merge priority. Attached to a config with `config:add_artifact`, which clones
/// the value into the config contribution for the plan service to fold.
/// It carries data for `add_artifact` to consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaArtifact {
    /// Materialization kind, half of the `(kind, path)` merge key.
    pub kind: ArtifactKind,
    /// Destination path, half of the `(kind, path)` merge key.
    pub path: String,
    /// Merged payload for the plan service.
    pub data: ArtifactData,
    /// Merge priority for the artifact, defaulting to 0.
    pub priority: u32,
}

impl UserData for LuaArtifact {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("with_priority", |_, this, value: Value| {
            this.priority = take_priority(value, "confit.artifact")?;
            Ok(this.clone())
        });
    }
}

/// Rc entry handle built by the `confit.artifact.rc` constructors.
///
/// Carries one rc payload plus guard plus priority. Tuned through `:when`
/// and `:with_priority`, submitted through `config:add_artifact`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RcEntry {
    /// Constructor label for method errors.
    pub ctor: &'static str,
    /// Payload selecting the entry shape.
    pub payload: RcPayload,
    /// Shell-session guard; `None` applies unconditionally.
    pub when: Option<Condition>,
    /// Merge priority for the entry, defaulting to 0.
    pub priority: u32,
}

/// Rc payload selecting one entry shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RcPayload {
    /// Alias name plus expansion.
    Alias { name: String, value: String },
    /// Variable name plus value.
    Env { name: String, value: String },
    /// Variable name plus directory, always prepended.
    Profile { name: String, value: String },
    /// Directory prepended to `PATH`.
    ProfilePath { dir: String },
    /// Init spec.
    Init { spec: RcSpec },
}

/// Rc init spec in model shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RcSpec {
    /// Command plus arguments evaluated via `eval "$(argv...)"`.
    Eval(Vec<String>),
    /// Command plus arguments run as a plain line.
    Cmd(Vec<String>),
    /// File path sourced via `source path`.
    Source(String),
}

impl UserData for RcEntry {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("when", |lua, this, value: Value| {
            let ctor = this.ctor;
            this.when = Some(parse_when_value(lua, ctor, value)?);
            Ok(this.clone())
        });
        methods.add_method_mut("with_priority", |_, this, value: Value| {
            let ctor = this.ctor;
            this.priority = take_priority(value, ctor)?;
            Ok(this.clone())
        });
    }
}

/// Reads a priority number from a Lua value.
///
/// # Arguments
///
/// * `value` - raw Lua value.
/// * `ctor` - constructor name for errors.
///
/// # Returns
///
/// Priority number.
///
/// # Errors
///
/// Fails with plan errors for values outside `u32` range.
fn take_priority(value: Value, ctor: &str) -> mlua::Result<u32> {
    match value {
        Value::Integer(number) => match u32::try_from(number) {
            Ok(priority) => Ok(priority),
            Err(_) => Err(rc_field_error(
                ctor,
                "priority",
                "must be an integer between 0 and 4294967295",
            )),
        },
        _ => Err(rc_field_error(
            ctor,
            "priority",
            "must be an integer between 0 and 4294967295",
        )),
    }
}

/// Installs the artifact namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the namespace.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
pub fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;

    register_table_constructor(
        lua,
        &namespace,
        "toml",
        "artifact.toml",
        ArtifactKind::Toml,
        ArtifactData::Toml,
    )?;
    register_table_constructor(
        lua,
        &namespace,
        "json",
        "artifact.json",
        ArtifactKind::Json,
        ArtifactData::Json,
    )?;
    register_table_constructor(
        lua,
        &namespace,
        "yaml",
        "artifact.yaml",
        ArtifactKind::Yaml,
        ArtifactData::Yaml,
    )?;
    register_string_constructor(
        lua,
        &namespace,
        "file",
        "artifact.file",
        "content",
        ArtifactKind::File,
        |content| ArtifactData::File { content },
    )?;
    let template = lua.create_function(|lua, args: (Value, Value)| template_impl(lua, args))?;
    namespace.set("template", template)?;
    register_string_constructor(
        lua,
        &namespace,
        "link",
        "artifact.link",
        "target",
        ArtifactKind::Link,
        |target| ArtifactData::Link { target },
    )?;
    install_rc(lua, &namespace)?;

    confit.set("artifact", namespace)?;
    Ok(())
}

/// Registers a table-payload constructor on the artifact namespace.
///
/// # Arguments
///
/// * `lua` - state owning the callback.
/// * `namespace` - table receiving the constructor.
/// * `name` - constructor name.
/// * `ctor` - constructor label for errors.
/// * `kind` - artifact kind for built values.
/// * `wrap` - wraps the data table as artifact payload.
///
/// # Errors
///
/// Fails with mlua errors for callback creation plus registration failures.
fn register_table_constructor(
    lua: &Lua,
    namespace: &Table,
    name: &str,
    ctor: &'static str,
    kind: ArtifactKind,
    wrap: fn(ModelTable) -> ArtifactData,
) -> mlua::Result<()> {
    let constructor = lua.create_function(move |_, args: (Value, Value)| {
        let (path, data) = args;
        let path = take_string(path, ctor, "path")?;
        let table = take_data_table(data, ctor, "data")?;
        Ok::<LuaArtifact, mlua::Error>(LuaArtifact {
            kind,
            path,
            data: wrap(table),
            priority: 0,
        })
    })?;
    namespace.set(name, constructor)
}

/// Registers a string-payload constructor on the artifact namespace.
///
/// # Arguments
///
/// * `lua` - state owning the callback.
/// * `namespace` - table receiving the constructor.
/// * `name` - constructor name.
/// * `ctor` - constructor label for errors.
/// * `field` - payload argument name for errors.
/// * `kind` - artifact kind for built values.
/// * `wrap` - wraps the payload string as artifact payload.
///
/// # Errors
///
/// Fails with mlua errors for callback creation plus registration failures.
fn register_string_constructor(
    lua: &Lua,
    namespace: &Table,
    name: &str,
    ctor: &'static str,
    field: &'static str,
    kind: ArtifactKind,
    wrap: fn(String) -> ArtifactData,
) -> mlua::Result<()> {
    let constructor = lua.create_function(move |_, args: (Value, Value)| {
        let (path, payload) = args;
        let path = take_string(path, ctor, "path")?;
        let payload = take_string(payload, ctor, field)?;
        Ok::<LuaArtifact, mlua::Error>(LuaArtifact {
            kind,
            path,
            data: wrap(payload),
            priority: 0,
        })
    })?;
    namespace.set(name, constructor)
}

/// Builds a field error naming constructor plus field.
///
/// # Arguments
///
/// * `ctor` - constructor name.
/// * `field` - Lua field.
/// * `detail` - violated expectation.
///
/// # Returns
///
/// Lua domain error naming constructor plus field.
fn field_error(ctor: &str, field: &str, detail: &str) -> mlua::Error {
    mlua::Error::external(Error::Lua(format!("{ctor}: field '{field}' {detail}")))
}

/// Reads a string argument from a Lua value.
///
/// # Arguments
///
/// * `value` - raw Lua value.
/// * `ctor` - constructor name for errors.
/// * `field` - argument name for errors.
///
/// # Returns
///
/// Argument string.
///
/// # Errors
///
/// Fails with Lua errors for values of other shapes.
fn take_string(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(field_error(ctor, field, "must be a string")),
    }
}

/// Reads a data table from a Lua value.
///
/// # Arguments
///
/// * `value` - raw Lua value.
/// * `ctor` - constructor name for errors.
/// * `field` - argument name for errors.
///
/// # Returns
///
/// Model table holding plain data.
///
/// # Errors
///
/// Fails with Lua errors for values of other shapes and for entries carrying executable shapes.
fn take_data_table(value: Value, ctor: &str, field: &str) -> mlua::Result<ModelTable> {
    let table = match value {
        Value::Table(table) => table,
        _ => return Err(field_error(ctor, field, "must be a table")),
    };
    let ctx = format!("{ctor}: field '{field}'");
    match table_to_json(&table, &ctx)? {
        Json::Object(map) => Ok(map.into_iter().collect::<BTreeMap<String, Json>>()),
        _ => Err(field_error(ctor, field, "must be a table with string keys")),
    }
}

/// Builds a template artifact value.
///
/// # Arguments
///
/// * `_lua` - callback state.
/// * `args` - path plus options values.
///
/// # Returns
///
/// Template artifact value.
///
/// # Errors
///
/// Fails with Lua errors for fields holding values of other shapes.
fn template_impl(_lua: &Lua, args: (Value, Value)) -> mlua::Result<LuaArtifact> {
    const CTOR: &str = "artifact.template";
    let (path, opts) = args;
    let path = take_string(path, CTOR, "path")?;
    let table = match opts {
        Value::Table(table) => table,
        _ => return Err(field_error(CTOR, "opts", "must be a table")),
    };
    let src: Value = table.get("src")?;
    let src = take_string(src, CTOR, "src")?;
    let vars: Value = table.get("vars")?;
    let vars = match vars {
        Value::Nil => BTreeMap::new(),
        value => take_data_table(value, CTOR, "vars")?,
    };
    Ok(LuaArtifact {
        kind: ArtifactKind::Template,
        path,
        data: ArtifactData::Template { src, vars },
        priority: 0,
    })
}

/// Converts a Lua value into JSON.
///
/// # Arguments
///
/// * `value` - Lua value.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON holding value data.
///
/// # Errors
///
/// Fails with Lua errors for values carrying executable shapes.
fn lua_to_json(value: Value, ctx: &str) -> mlua::Result<Json> {
    match value {
        Value::Nil => Ok(Json::Null),
        Value::Boolean(flag) => Ok(Json::Bool(flag)),
        Value::Integer(number) => Ok(Json::Number(number.into())),
        Value::Number(number) => match serde_json::Number::from_f64(number) {
            Some(parsed) => Ok(Json::Number(parsed)),
            None => Err(lua_err!("{ctx} must be a finite number")),
        },
        Value::String(text) => Ok(Json::String(text.to_string_lossy())),
        Value::Table(table) => table_to_json(&table, ctx),
        Value::Function(_) => Err(lua_err!("{ctx} must be data-only (function not allowed)")),
        Value::UserData(_) => Err(lua_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::LightUserData(_) => Err(lua_err!("{ctx} must be data-only (userdata not allowed)")),
        Value::Thread(_) => Err(lua_err!("{ctx} must be data-only (thread not allowed)")),
        Value::Error(_) => Err(lua_err!("{ctx} must be data-only")),
        Value::Other(_) => Err(lua_err!("{ctx} must be data-only")),
    }
}

/// Converts a Lua table into a JSON value.
///
/// # Arguments
///
/// * `table` - Lua table.
/// * `ctx` - field path for errors.
///
/// # Returns
///
/// JSON array for dense arrays, else JSON object.
///
/// # Errors
///
/// Fails with Lua errors for keys of other shapes and for values carrying executable shapes.
fn table_to_json(table: &Table, ctx: &str) -> mlua::Result<Json> {
    let mut entries: Vec<(Value, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        entries.push(pair?);
    }
    if entries.is_empty() {
        return Ok(Json::Object(serde_json::Map::new()));
    }
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    let mut all_integer = true;
    for (key, value) in &entries {
        match key {
            Value::Integer(index) => indexed.push((*index, value.clone())),
            _ => {
                all_integer = false;
                break;
            }
        }
    }
    if all_integer {
        indexed.sort_by_key(|(index, _)| *index);
        let dense = indexed
            .iter()
            .enumerate()
            .all(|(position, (index, _))| *index == position as i64 + 1);
        if dense {
            let mut items = Vec::with_capacity(indexed.len());
            for (index, value) in &indexed {
                let child = format!("{ctx}[{index}]");
                items.push(lua_to_json(value.clone(), &child)?);
            }
            return Ok(Json::Array(items));
        }
    }
    let mut map = serde_json::Map::new();
    for (key, value) in &entries {
        let name = match key {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(lua_err!("{ctx} must be a table with string keys"));
            }
        };
        let child = format!("{ctx}.{name}");
        map.insert(name, lua_to_json(value.clone(), &child)?);
    }
    Ok(Json::Object(map))
}

/// Installs the rc entry constructors on the artifact namespace.
///
/// # Arguments
///
/// * `lua` - state owning the callbacks.
/// * `namespace` - artifact table receiving the `rc` subtable.
///
/// # Errors
///
/// Fails with mlua errors for table creation plus registration failures.
fn install_rc(lua: &Lua, namespace: &Table) -> mlua::Result<()> {
    let rc = lua.create_table()?;
    rc.set("alias", lua.create_function(rc_alias_impl)?)?;
    rc.set("env", lua.create_function(rc_env_impl)?)?;
    rc.set("profile", lua.create_function(rc_profile_impl)?)?;
    rc.set("profile_path", lua.create_function(rc_profile_path_impl)?)?;
    rc.set("init", lua.create_function(rc_init_impl)?)?;
    namespace.set("rc", rc)
}

/// Builds a plan error naming constructor plus field.
fn rc_field_error(ctor: &str, field: &str, detail: &str) -> mlua::Error {
    mlua::Error::external(Error::Plan(format!("{ctor}: field '{field}' {detail}")))
}

/// Reads a string from a Lua value as a plan error.
fn take_rc_string(value: Value, ctor: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(rc_field_error(ctor, field, "must be a string")),
    }
}

/// Builds one rc alias entry handle.
fn rc_alias_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<RcEntry> {
    const CTOR: &str = "confit.artifact.rc.alias";
    let (name, value, opts) = args;
    let name = take_rc_string(name, CTOR, "name")?;
    let value = take_rc_string(value, CTOR, "value")?;
    let (when, priority) = rc_opts(lua, CTOR, opts)?;
    Ok(RcEntry {
        ctor: CTOR,
        payload: RcPayload::Alias { name, value },
        when,
        priority,
    })
}

/// Builds one rc env entry handle.
fn rc_env_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<RcEntry> {
    const CTOR: &str = "confit.artifact.rc.env";
    let (name, value, opts) = args;
    let name = take_rc_string(name, CTOR, "name")?;
    let value = take_rc_string(value, CTOR, "value")?;
    let (when, priority) = rc_opts(lua, CTOR, opts)?;
    Ok(RcEntry {
        ctor: CTOR,
        payload: RcPayload::Env { name, value },
        when,
        priority,
    })
}

/// Builds one rc profile entry handle.
fn rc_profile_impl(lua: &Lua, args: (Value, Value, Value)) -> mlua::Result<RcEntry> {
    const CTOR: &str = "confit.artifact.rc.profile";
    let (name, value, opts) = args;
    let name = take_rc_string(name, CTOR, "name")?;
    let value = take_rc_string(value, CTOR, "value")?;
    let (when, priority) = rc_opts(lua, CTOR, opts)?;
    Ok(RcEntry {
        ctor: CTOR,
        payload: RcPayload::Profile { name, value },
        when,
        priority,
    })
}

/// Builds one rc profile_path entry handle.
fn rc_profile_path_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<RcEntry> {
    const CTOR: &str = "confit.artifact.rc.profile_path";
    let (dir, opts) = args;
    let dir = take_rc_string(dir, CTOR, "dir")?;
    let (when, priority) = rc_opts(lua, CTOR, opts)?;
    Ok(RcEntry {
        ctor: CTOR,
        payload: RcPayload::ProfilePath { dir },
        when,
        priority,
    })
}

/// Builds one rc init entry handle.
fn rc_init_impl(lua: &Lua, args: (Value, Value)) -> mlua::Result<RcEntry> {
    const CTOR: &str = "confit.artifact.rc.init";
    let (spec, opts) = args;
    let spec = parse_rc_spec(spec, CTOR)?;
    let (when, priority) = rc_opts(lua, CTOR, opts)?;
    Ok(RcEntry {
        ctor: CTOR,
        payload: RcPayload::Init { spec },
        when,
        priority,
    })
}

/// Resolves the optional `when` guard plus `priority` from an opts value.
fn rc_opts(lua: &Lua, ctor: &str, opts: Value) -> mlua::Result<(Option<Condition>, u32)> {
    let table = match opts {
        Value::Nil => return Ok((None, 0)),
        Value::Table(table) => table,
        _ => return Err(rc_field_error(ctor, "opts", "must be a table")),
    };
    let when_value: Value = table.get("when")?;
    let when = match when_value {
        Value::Nil => None,
        value => Some(parse_when_value(lua, ctor, value)?),
    };
    let priority_value: Value = table.get("priority")?;
    let priority = match priority_value {
        Value::Nil => 0,
        value => take_priority(value, ctor)?,
    };
    Ok((when, priority))
}

/// Resolves one `when` value into model shape.
fn parse_when_value(lua: &Lua, ctor: &str, when: Value) -> mlua::Result<Condition> {
    match when {
        Value::Table(table) => parse_condition(ctor, "when", &table),
        Value::Function(func) => {
            let table = call_when_function(lua, ctor, &func)?;
            parse_condition(ctor, "when", &table)
        }
        _ => Err(rc_field_error(
            ctor,
            "when",
            "must be a condition table or function",
        )),
    }
}

/// Reads one condition table into model shape.
fn parse_condition(ctx: &str, field: &str, table: &Table) -> mlua::Result<Condition> {
    let mut entries: Vec<(String, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            Value::String(text) => entries.push((text.to_string_lossy(), value)),
            _ => {
                return Err(rc_field_error(
                    ctx,
                    field,
                    "must be a condition table with one shape",
                ));
            }
        }
    }
    if entries.len() != 1 {
        return Err(rc_field_error(
            ctx,
            field,
            "must be a condition table with one shape",
        ));
    }
    let (shape, inner) = match entries.pop() {
        Some(pair) => pair,
        None => {
            return Err(rc_field_error(
                ctx,
                field,
                "must be a condition table with one shape",
            ));
        }
    };
    match shape.as_str() {
        "env_eq" => {
            let payload = condition_table(ctx, field, &inner, "env_eq")?;
            let key = condition_string(&payload, ctx, field, "key", "env_eq")?;
            let value = condition_string(&payload, ctx, field, "value", "env_eq")?;
            Ok(Condition::EnvEq { key, value })
        }
        "env_set" => {
            let payload = condition_table(ctx, field, &inner, "env_set")?;
            let key = condition_string(&payload, ctx, field, "key", "env_set")?;
            Ok(Condition::EnvSet { key })
        }
        "in_path" => {
            let payload = condition_table(ctx, field, &inner, "in_path")?;
            let name = condition_string(&payload, ctx, field, "name", "in_path")?;
            Ok(Condition::InPath { name })
        }
        "exists" => {
            let payload = condition_table(ctx, field, &inner, "exists")?;
            let path = condition_string(&payload, ctx, field, "path", "exists")?;
            Ok(Condition::Exists { path })
        }
        "all" | "any" => {
            let nested = format!("{field}.{shape}");
            let items = parse_condition_array(ctx, &nested, &inner)?;
            if shape == "all" {
                Ok(Condition::All(items))
            } else {
                Ok(Condition::Any(items))
            }
        }
        "nop" => {
            let nested = format!("{field}.nop");
            match inner {
                Value::Table(table) => {
                    parse_condition(ctx, &nested, &table).map(|item| Condition::Not(Box::new(item)))
                }
                _ => Err(rc_field_error(ctx, &nested, "must be a condition table")),
            }
        }
        _ => Err(rc_field_error(
            ctx,
            field,
            "must be a condition table with one shape",
        )),
    }
}

/// Reads one condition payload table for a shape.
fn condition_table(ctx: &str, field: &str, inner: &Value, shape: &str) -> mlua::Result<Table> {
    match inner {
        Value::Table(table) => Ok(table.clone()),
        _ => Err(rc_field_error(
            ctx,
            field,
            &format!("entry '{shape}' must be a table"),
        )),
    }
}

/// Reads one string field from a condition payload table.
fn condition_string(
    payload: &Table,
    ctx: &str,
    field: &str,
    name: &str,
    shape: &str,
) -> mlua::Result<String> {
    let raw: Value = payload.get(name)?;
    match raw {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(rc_field_error(
            ctx,
            field,
            &format!("entry '{shape}' field '{name}' must be a string"),
        )),
    }
}

/// Reads one dense condition array into model shape.
fn parse_condition_array(ctx: &str, field: &str, inner: &Value) -> mlua::Result<Vec<Condition>> {
    let table = match inner {
        Value::Table(table) => table.clone(),
        _ => {
            return Err(rc_field_error(ctx, field, "must be a condition array"));
        }
    };
    let mut indexed: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        match key {
            Value::Integer(index) => indexed.push((index, value)),
            _ => {
                return Err(rc_field_error(
                    ctx,
                    field,
                    "must be a dense condition array starting at 1",
                ));
            }
        }
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in (1i64..).zip(indexed.iter()) {
        if *index != position {
            return Err(rc_field_error(
                ctx,
                field,
                "must be a dense condition array starting at 1",
            ));
        }
    }
    let mut out = Vec::with_capacity(indexed.len());
    for (index, value) in &indexed {
        let child = format!("{field}[{index}]");
        match value {
            Value::Table(table) => out.push(parse_condition(ctx, &child, table)?),
            _ => {
                return Err(rc_field_error(ctx, &child, "must be a condition table"));
            }
        }
    }
    Ok(out)
}

/// Parses one rc init spec into model shape.
fn parse_rc_spec(spec: Value, ctor: &str) -> mlua::Result<RcSpec> {
    let table = match spec {
        Value::Table(table) => table,
        _ => {
            return Err(rc_field_error(
                ctor,
                "spec",
                "must be a table with exactly one of 'eval', 'cmd', or 'source'",
            ));
        }
    };
    let eval: Value = table.get("eval")?;
    let cmd: Value = table.get("cmd")?;
    let source: Value = table.get("source")?;
    match (eval, cmd, source) {
        (Value::Table(argv), Value::Nil, Value::Nil) => {
            let argv = take_model_string_array(&argv, ctor, "spec.eval")?;
            Ok(RcSpec::Eval(argv))
        }
        (Value::Nil, Value::Table(argv), Value::Nil) => {
            let argv = take_model_string_array(&argv, ctor, "spec.cmd")?;
            Ok(RcSpec::Cmd(argv))
        }
        (Value::Nil, Value::Nil, Value::String(path)) => Ok(RcSpec::Source(path.to_string_lossy())),
        _ => Err(rc_field_error(
            ctor,
            "spec",
            "must hold exactly one of 'eval' or 'cmd' with a string array, or 'source' with a string path",
        )),
    }
}

/// Reads a dense string array into model shape.
fn take_model_string_array(table: &Table, ctor: &str, field: &str) -> mlua::Result<Vec<String>> {
    let mut indexed: Vec<(i64, String)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, value) = pair?;
        let index = match key {
            Value::Integer(index) => index,
            _ => return Err(rc_field_error(ctor, field, "must be a string array")),
        };
        let item = match value {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(rc_field_error(
                    ctor,
                    field,
                    &format!("entry [{index}] must be a string"),
                ));
            }
        };
        indexed.push((index, item));
    }
    indexed.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in (1i64..).zip(indexed.iter()) {
        if *index != position {
            return Err(rc_field_error(
                ctor,
                field,
                "must be a dense string array starting at 1",
            ));
        }
    }
    Ok(indexed.into_iter().map(|(_, item)| item).collect())
}

/// Calls a `when` builder function with the shell namespace.
fn call_when_function(lua: &Lua, ctor: &str, func: &mlua::Function) -> mlua::Result<Table> {
    let shell = match lua.globals().get::<Value>("confit")? {
        Value::Table(confit) => match confit.get::<Value>("shell")? {
            Value::Table(shell) => shell,
            _ => return Err(rc_field_error(ctor, "when", "needs the confit.shell table")),
        },
        _ => return Err(rc_field_error(ctor, "when", "needs the confit.shell table")),
    };
    match func.call::<Table>(shell) {
        Ok(table) => Ok(table),
        Err(err) => {
            let is_plan = err
                .downcast_ref::<Error>()
                .is_some_and(|domain| matches!(domain, Error::Plan(_)));
            if is_plan {
                return Err(err);
            }
            Err(rc_field_error(
                ctor,
                "when",
                &format!("function failed: {err}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a Lua state holding artifact constructors.
    ///
    /// # Returns
    ///
    /// Lua state holding the constructors.
    fn setup() -> Lua {
        let lua = Lua::new();
        install(&lua).expect("install");
        lua
    }

    /// Builds an artifact value from a Lua chunk.
    ///
    /// # Arguments
    ///
    /// * `lua` - prepared state.
    /// * `expr` - Lua chunk returning one artifact.
    ///
    /// # Returns
    ///
    /// Artifact value holding chunk data.
    fn eval_artifact(lua: &Lua, expr: &str) -> LuaArtifact {
        let value: Value = lua.load(expr).eval().expect("evaluate");
        match value {
            Value::UserData(handle) => handle.borrow::<LuaArtifact>().expect("artifact").clone(),
            other => panic!("expected artifact userdata, got {other:?}"),
        }
    }

    /// Reports Lua domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for Lua domain failures, false for other failures.
    fn is_lua_error(err: &mlua::Error) -> bool {
        if err.downcast_ref::<Error>().is_some() {
            return true;
        }
        format!("{err}").contains("lua error:")
    }

    #[test]
    fn constructors_cover_all_kinds() {
        let lua = setup();
        let cases: Vec<(&str, ArtifactKind, &str)> = vec![
            (
                r#"return confit.artifact.toml("a.toml", {x = 1})"#,
                ArtifactKind::Toml,
                "a.toml",
            ),
            (
                r#"return confit.artifact.json("a.json", {x = 1})"#,
                ArtifactKind::Json,
                "a.json",
            ),
            (
                r#"return confit.artifact.yaml("a.yaml", {x = 1})"#,
                ArtifactKind::Yaml,
                "a.yaml",
            ),
            (
                r#"return confit.artifact.file("a.txt", "hi")"#,
                ArtifactKind::File,
                "a.txt",
            ),
            (
                r#"return confit.artifact.template("a.out", {src = "s", vars = {x = 1}})"#,
                ArtifactKind::Template,
                "a.out",
            ),
            (
                r#"return confit.artifact.link("a.link", "target")"#,
                ArtifactKind::Link,
                "a.link",
            ),
        ];
        for (expr, kind, path) in cases {
            let artifact = eval_artifact(&lua, expr);
            assert_eq!(artifact.kind, kind, "{expr}");
            assert_eq!(artifact.path, path, "{expr}");
        }
        let toml = eval_artifact(&lua, r#"return confit.artifact.toml("a.toml", {x = 1})"#);
        match toml.data {
            ArtifactData::Toml(table) => {
                assert_eq!(table.get("x").and_then(Json::as_i64), Some(1));
            }
            other => panic!("expected toml data, got {other:?}"),
        }
        let file = eval_artifact(&lua, r#"return confit.artifact.file("a.txt", "hi")"#);
        assert_eq!(
            file.data,
            ArtifactData::File {
                content: "hi".to_string()
            }
        );
        let template = eval_artifact(
            &lua,
            r#"return confit.artifact.template("a.out", {src = "s"})"#,
        );
        match template.data {
            ArtifactData::Template { src, vars } => {
                assert_eq!(src, "s");
                assert!(vars.is_empty());
            }
            other => panic!("expected template data, got {other:?}"),
        }
    }

    #[test]
    fn data_only_validation_names_field() {
        let lua = setup();
        for (field, expr) in [
            (
                "data",
                r#"return confit.artifact.toml("p", {f = function() end})"#,
            ),
            (
                "data",
                r#"return confit.artifact.json("p", {{function() end}})"#,
            ),
            ("path", r#"return confit.artifact.file(42, "hi")"#),
            ("content", r#"return confit.artifact.file("p", 42)"#),
            ("src", r#"return confit.artifact.template("p", {})"#),
            (
                "vars",
                r#"return confit.artifact.template("p", {src = "s", vars = 42})"#,
            ),
            ("target", r#"return confit.artifact.link("p", {})"#),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_lua_error(&err), "Lua domain: {expr}: {err}");
            assert!(
                format!("{err}").contains(field),
                "names field {field}: {expr}: {err}"
            );
        }
    }

    /// Builds a Lua state holding artifact plus shell namespaces.
    ///
    /// # Returns
    ///
    /// Lua state holding both namespaces.
    fn setup_rc() -> Lua {
        let lua = Lua::new();
        install(&lua).expect("install artifact");
        crate::framework::shell::install(&lua).expect("install shell");
        lua
    }

    /// Evaluates a Lua chunk returning one rc entry handle.
    ///
    /// # Arguments
    ///
    /// * `lua` - prepared state.
    /// * `expr` - Lua chunk returning one entry handle.
    ///
    /// # Returns
    ///
    /// Entry handle holding chunk data.
    fn eval_rc_entry(lua: &Lua, expr: &str) -> RcEntry {
        let value: Value = lua.load(expr).eval().expect("evaluate");
        match value {
            Value::UserData(handle) => handle.borrow::<RcEntry>().expect("entry").clone(),
            other => panic!("expected rc entry userdata, got {other:?}"),
        }
    }

    /// Reports plan domain status for a Lua failure.
    ///
    /// # Arguments
    ///
    /// * `err` - Lua failure.
    ///
    /// # Returns
    ///
    /// True for plan domain failures, false for other failures.
    fn is_plan_error(err: &mlua::Error) -> bool {
        if let Some(domain) = err.downcast_ref::<Error>() {
            return matches!(domain, Error::Plan(_));
        }
        format!("{err}").contains("plan error:")
    }

    #[test]
    fn rc_entries_carry_payload_without_guard_or_priority() {
        let lua = setup_rc();
        let alias = eval_rc_entry(&lua, r#"return confit.artifact.rc.alias("cat", "bat")"#);
        assert!(matches!(
            alias.payload,
            RcPayload::Alias { ref name, ref value } if name == "cat" && value == "bat"
        ));
        assert!(alias.when.is_none());
        assert_eq!(alias.priority, 0);

        let env = eval_rc_entry(&lua, r#"return confit.artifact.rc.env("A", "1")"#);
        assert!(matches!(
            env.payload,
            RcPayload::Env { ref name, ref value } if name == "A" && value == "1"
        ));

        let profile = eval_rc_entry(&lua, r#"return confit.artifact.rc.profile("P", "v")"#);
        assert!(matches!(
            profile.payload,
            RcPayload::Profile { ref name, ref value } if name == "P" && value == "v"
        ));

        let dir = eval_rc_entry(&lua, r#"return confit.artifact.rc.profile_path("/bin")"#);
        assert!(matches!(
            dir.payload,
            RcPayload::ProfilePath { ref dir } if dir == "/bin"
        ));
        assert!(dir.when.is_none());

        let init = eval_rc_entry(
            &lua,
            r#"return confit.artifact.rc.init({eval = {"mise", "activate"}})"#,
        );
        assert!(matches!(
            init.payload,
            RcPayload::Init { ref spec } if spec == &RcSpec::Eval(vec!["mise".into(), "activate".into()])
        ));
        assert!(init.when.is_none());
    }

    #[test]
    fn rc_init_covers_cmd_and_source_shapes() {
        let lua = setup_rc();
        let cmd = eval_rc_entry(&lua, r#"return confit.artifact.rc.init({cmd = {"task"}})"#);
        assert!(matches!(
            cmd.payload,
            RcPayload::Init { spec: RcSpec::Cmd(ref argv) } if argv == &vec!["task".to_string()]
        ));
        let sourced = eval_rc_entry(&lua, r#"return confit.artifact.rc.init({source = "x"})"#);
        assert!(matches!(
            sourced.payload,
            RcPayload::Init { spec: RcSpec::Source(ref path) } if path == "x"
        ));
    }

    #[test]
    fn rc_methods_tune_guard_and_priority() {
        let lua = setup_rc();
        let entry = eval_rc_entry(
            &lua,
            r#"return confit.artifact.rc.alias("cat", "bat"):when({in_path = {name = "bat"}}):with_priority(5)"#,
        );
        assert!(matches!(
            entry.when,
            Some(Condition::InPath { ref name }) if name == "bat"
        ));
        assert_eq!(entry.priority, 5);

        let opted = eval_rc_entry(
            &lua,
            r#"return confit.artifact.rc.env("A", "1", {when = {env_set = {key = "X"}}, priority = 3})"#,
        );
        assert!(matches!(
            opted.when,
            Some(Condition::EnvSet { ref key }) if key == "X"
        ));
        assert_eq!(opted.priority, 3);

        let file: LuaArtifact = {
            let value: Value = lua
                .load(r#"return confit.artifact.file("a.txt", "hi"):with_priority(9)"#)
                .eval()
                .expect("evaluate");
            match value {
                Value::UserData(handle) => {
                    handle.borrow::<LuaArtifact>().expect("artifact").clone()
                }
                other => panic!("expected artifact userdata, got {other:?}"),
            }
        };
        assert_eq!(file.priority, 9);
    }

    #[test]
    fn rc_when_table_parses_to_model_shape() {
        let lua = setup_rc();
        let entry = eval_rc_entry(
            &lua,
            r#"return confit.artifact.rc.env("A", "1", {when = confit.shell.env_eq({key = "K", value = "v"})})"#,
        );
        assert!(matches!(
            entry.when,
            Some(Condition::EnvEq { ref key, ref value }) if key == "K" && value == "v"
        ));

        let nested = eval_rc_entry(
            &lua,
            r#"local s = confit.shell
            return confit.artifact.rc.env("A", "1", {when = function(c)
                return c.all({c.env_set({key = "X"}), c.nop(c.env_set({key = "Y"}))})
            end})"#,
        );
        assert!(matches!(nested.when, Some(Condition::All(_))));
    }

    #[test]
    fn rc_when_function_receives_shell_table() {
        let lua = setup_rc();
        let entry = eval_rc_entry(
            &lua,
            r#"return confit.artifact.rc.alias("cat", "bat", {when = function(s) return s.env_set({key = "X"}) end})"#,
        );
        assert!(matches!(
            entry.when,
            Some(Condition::EnvSet { ref key }) if key == "X"
        ));
    }

    #[test]
    fn rc_mistakes_fail_as_named_plan_errors() {
        let lua = setup_rc();
        for (ctor, field, expr) in [
            (
                "confit.artifact.rc.alias",
                "name",
                r#"return confit.artifact.rc.alias(42, "bat")"#,
            ),
            (
                "confit.artifact.rc.alias",
                "value",
                r#"return confit.artifact.rc.alias("cat", 42)"#,
            ),
            (
                "confit.artifact.rc.alias",
                "opts",
                r#"return confit.artifact.rc.alias("cat", "bat", 42)"#,
            ),
            (
                "confit.artifact.rc.alias",
                "when",
                r#"return confit.artifact.rc.alias("cat", "bat", {when = 42})"#,
            ),
            (
                "confit.artifact.rc.env",
                "when",
                r#"return confit.artifact.rc.env("A", "1", {when = {bogus = {}}})"#,
            ),
            (
                "confit.artifact.rc.env",
                "when",
                r#"return confit.artifact.rc.env("A", "1", {when = {env_eq = {key = "K"}}})"#,
            ),
            (
                "confit.artifact.rc.profile",
                "when",
                r#"return confit.artifact.rc.profile("P", "v", {when = {in_path = {name = 42}}})"#,
            ),
            (
                "confit.artifact.rc.profile_path",
                "dir",
                r#"return confit.artifact.rc.profile_path(42)"#,
            ),
            (
                "confit.artifact.rc.profile_path",
                "when",
                r#"return confit.artifact.rc.profile_path("/bin", {when = {exists = {}}})"#,
            ),
            (
                "confit.artifact.rc.init",
                "spec",
                r#"return confit.artifact.rc.init({eval = {"a"}, cmd = {"b"}})"#,
            ),
            (
                "confit.artifact.rc.init",
                "spec",
                r#"return confit.artifact.rc.init({source = 42})"#,
            ),
            (
                "confit.artifact.rc.init",
                "spec.eval",
                r#"return confit.artifact.rc.init({eval = {"a", 42}})"#,
            ),
            (
                "confit.artifact.rc.init",
                "when",
                r#"return confit.artifact.rc.init({cmd = {"a"}}, {when = function(s) return 42 end})"#,
            ),
        ] {
            let err = lua.load(expr).eval::<Value>().expect_err("must fail");
            assert!(is_plan_error(&err), "plan domain: {expr}: {err}");
            let message = format!("{err}");
            assert!(message.contains(ctor), "names {ctor}: {expr}: {err}");
            assert!(message.contains(field), "names {field}: {expr}: {err}");
        }
    }
}
