//! Artifact
//!
//! Artifact value constructors for Lua.

use std::collections::BTreeMap;

use mlua::{Lua, Table, UserData, Value};
use serde_json::Value as Json;

use crate::error::Error;
use crate::model::state::artifact::ArtifactData;
use crate::model::state::artifact::ArtifactKind;
use crate::model::state::artifact::Table as ModelTable;

use super::confit_table;

/// Opaque artifact value built by the `confit.artifact` constructors.
///
/// Carries the merge key (`kind`, `path`) plus the payload (`data`).
/// Attached to a tool with `tool:append_artifact`, which clones the
/// value into the tool contribution for the plan service to fold.
/// It carries data for `append_artifact` to consume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LuaArtifact {
    /// Materialization kind, half of the `(kind, path)` merge key.
    pub kind: ArtifactKind,
    /// Destination path, half of the `(kind, path)` merge key.
    pub path: String,
    /// Merged payload for the plan service.
    pub data: ArtifactData,
}

impl UserData for LuaArtifact {}

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
        })
    })?;
    namespace.set(name, constructor)
}

/// Fetches the confit global table for a Lua state.
///
/// # Arguments
///
/// * `lua` - state holding the global.
///
/// # Returns
///
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
}
