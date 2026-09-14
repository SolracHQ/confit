//! Plugin
//!
//! Lazy user plugin namespaces plus embedded defaults.

use std::path::{Component, Path, PathBuf};

use mlua::{Function, Lua, Table, Value};

use super::confit_table;
use crate::values::plan_error;

/// Registry key holding loaded plugins by `user/name`.
const LOADED_KEY: &str = "confit.plugin.loaded";
/// Registry key holding user namespace tables.
const USERS_KEY: &str = "confit.plugin.users";
/// Registry key holding scoped require results by file.
const REQUIRE_CACHE_KEY: &str = "confit.plugin.require_cache";

/// Embedded default plugin sources in external shape.
const EMBEDDED: &[(&str, &str, &str)] = &[
    (
        "solrachq",
        "mise",
        include_str!("../../plugins/solrachq/mise/plugin.lua"),
    ),
    (
        "solrachq",
        "merge",
        include_str!("../../plugins/solrachq/merge/plugin.lua"),
    ),
    (
        "solrachq",
        "template",
        include_str!("../../plugins/solrachq/template/plugin.lua"),
    ),
];

/// Installs the plugin namespace on a state.
pub(crate) fn install(lua: &Lua, plugins: Option<&Path>) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    let helpers = lua.create_table()?;
    helpers.set(
        "error",
        lua.create_function(|_, message: Value| helpers_error_impl(message))?,
    )?;
    namespace.set("helpers", helpers)?;
    lua.set_named_registry_value(LOADED_KEY, lua.create_table()?)?;
    lua.set_named_registry_value(USERS_KEY, lua.create_table()?)?;
    lua.set_named_registry_value(REQUIRE_CACHE_KEY, lua.create_table()?)?;
    let root = plugins.map(Path::to_path_buf);
    let meta = lua.create_table()?;
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, key): (Table, Value)| {
            users_index(lua, key, root.clone())
        })?,
    )?;
    namespace.set_metatable(Some(meta))?;
    confit.set("plugin", namespace)?;
    Ok(())
}

/// Resolves one username into a lazy user namespace.
fn users_index(lua: &Lua, key: Value, root: Option<PathBuf>) -> mlua::Result<Value> {
    let user = match key {
        Value::String(text) => text.to_string_lossy(),
        _ => return Ok(Value::Nil),
    };
    let users: Table = lua.named_registry_value(USERS_KEY)?;
    if let Value::Table(cached) = users.get::<Value>(user.as_str())? {
        return Ok(Value::Table(cached));
    }
    let table = lua.create_table()?;
    let meta = lua.create_table()?;
    let user_clone = user.clone();
    meta.set(
        "__index",
        lua.create_function(move |lua, (_, name): (Table, Value)| {
            names_index(lua, &user_clone, name, root.clone())
        })?,
    )?;
    table.set_metatable(Some(meta))?;
    users.set(user.as_str(), table.clone())?;
    Ok(Value::Table(table))
}

/// Resolves one plugin name into its loaded return value.
fn names_index(lua: &Lua, user: &str, key: Value, root: Option<PathBuf>) -> mlua::Result<Value> {
    let name = match key {
        Value::String(text) => text.to_string_lossy(),
        _ => return Ok(Value::Nil),
    };
    let loaded: Table = lua.named_registry_value(LOADED_KEY)?;
    let slot = format!("{user}/{name}");
    if let value @ (Value::Table(_) | Value::Function(_) | Value::String(_)) =
        loaded.get::<Value>(slot.as_str())?
    {
        return Ok(value);
    }
    let embedded = EMBEDDED
        .iter()
        .find(|(owner, plugin, _)| *owner == user && *plugin == name)
        .map(|(_, _, source)| *source);
    let external = root
        .as_ref()
        .map(|folder| folder.join(user).join(&name).join("plugin.lua"))
        .filter(|file| file.is_file());
    if embedded.is_some() && external.is_some() {
        eprintln!("confit.plugin.{user}.{name}: embedded default wins, external plugin skipped");
    }
    let value = match embedded {
        Some(source) => {
            let chunk = lua
                .load(source)
                .set_name(format!("@plugins/{user}/{name}/plugin.lua"));
            chunk.call(())?
        }
        None => match external {
            Some(file) => load_external(lua, user, &name, &file)?,
            None => {
                return Err(plan_error(format!(
                    "confit.plugin.{user}.{name}: cannot read 'plugin.lua': no such plugin"
                )));
            }
        },
    };
    loaded.set(slot.as_str(), value.clone())?;
    Ok(value)
}

/// Loads one external plugin file with a scoped require.
fn load_external(lua: &Lua, user: &str, name: &str, file: &Path) -> mlua::Result<Value> {
    let folder = match file.parent() {
        Some(parent) => parent.to_path_buf(),
        None => {
            return Err(plan_error(format!(
                "confit.plugin.{user}.{name}: cannot read 'plugin.lua': bad path"
            )));
        }
    };
    let source = std::fs::read_to_string(file).map_err(|error| {
        plan_error(format!(
            "confit.plugin.{user}.{name}: cannot read 'plugin.lua': {error}"
        ))
    })?;
    let requirer = make_require(lua, folder.clone(), folder.clone())?;
    let env = chunk_env(lua, requirer)?;
    let chunk = lua
        .load(&source)
        .set_name(format!("@{}", file.display()))
        .set_environment(env);
    chunk
        .call(())
        .map_err(|error| plan_error(format!("confit.plugin.{user}.{name}: {error}")))
}

/// Builds one chunk environment holding a scoped require.
fn chunk_env(lua: &Lua, requirer: Function) -> mlua::Result<Table> {
    let env = lua.create_table()?;
    env.set("require", requirer)?;
    let fallback = lua.create_table()?;
    fallback.set("__index", lua.globals())?;
    env.set_metatable(Some(fallback))?;
    Ok(env)
}

/// Builds one scoped require resolving inside a plugin folder.
fn make_require(lua: &Lua, current: PathBuf, folder: PathBuf) -> mlua::Result<Function> {
    lua.create_function(move |lua, request: Value| {
        let request = match request {
            Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(plan_error(
                    "plugin.require: field 'module' must be a string".to_string(),
                ));
            }
        };
        require_impl(lua, &current, &folder, &request)
    })
}

/// Resolves one scoped require request into a cached value.
fn require_impl(lua: &Lua, current: &Path, folder: &Path, request: &str) -> mlua::Result<Value> {
    const CALLER: &str = "plugin.require";
    let file = resolve_require(current, folder, request)?;
    let cache: Table = lua.named_registry_value(REQUIRE_CACHE_KEY)?;
    let slot = file.display().to_string();
    if let value @ (Value::Table(_) | Value::Function(_) | Value::String(_)) =
        cache.get::<Value>(slot.as_str())?
    {
        return Ok(value);
    }
    let source = std::fs::read_to_string(&file)
        .map_err(|error| plan_error(format!("{CALLER}: cannot read '{request}': {error}")))?;
    let parent = file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| folder.to_path_buf());
    let requirer = make_require(lua, parent, folder.to_path_buf())?;
    let env = chunk_env(lua, requirer)?;
    let value: Value = lua
        .load(&source)
        .set_name(format!("@{}", file.display()))
        .set_environment(env)
        .call(())?;
    cache.set(slot.as_str(), value.clone())?;
    Ok(value)
}

/// Resolves one scoped require request inside the plugin folder.
fn resolve_require(current: &Path, folder: &Path, request: &str) -> mlua::Result<PathBuf> {
    const CALLER: &str = "plugin.require";
    if request.is_empty() {
        return Err(plan_error(format!(
            "{CALLER}: field 'module' must not be empty"
        )));
    }
    let mut stack: Vec<String> = Vec::new();
    for part in request.split('.') {
        if part.is_empty() {
            return Err(plan_error(format!("{CALLER}: bad module '{request}'")));
        }
        stack.push(part.to_string());
    }
    let mut base = current.to_path_buf();
    for part in &stack {
        base.push(part);
    }
    base.set_extension("lua");
    let mut normalized = PathBuf::new();
    for component in base.components() {
        match component {
            Component::ParentDir => {
                if !normalized.pop() {
                    return Err(plan_error(format!(
                        "{CALLER}: module '{request}' escapes the plugin folder"
                    )));
                }
            }
            Component::CurDir => {}
            other => normalized.push(other),
        }
    }
    let root = folder.components().collect::<PathBuf>();
    if !normalized.starts_with(&root) {
        return Err(plan_error(format!(
            "{CALLER}: module '{request}' escapes the plugin folder"
        )));
    }
    Ok(normalized)
}

/// Raises one plan error with plugin attribution.
fn helpers_error_impl(message: Value) -> mlua::Result<Value> {
    const CALLER: &str = "confit.plugin.helpers.error";
    match message {
        Value::String(text) => Err(plan_error(format!("{CALLER}: {}", text.to_string_lossy()))),
        _ => Err(plan_error(format!(
            "{CALLER}: field 'message' must be a string"
        ))),
    }
}
