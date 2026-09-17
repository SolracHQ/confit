//! Plugin
//!
//! Lazy user plugin namespaces plus embedded defaults.

use std::path::{Path, PathBuf};

use mlua::{Lua, Table, Value};

use super::confit_table;
use crate::error::plan_error;
use crate::lua::ValueExt;
use crate::require::{Requirer, chunk_env};

/// Registry key holding loaded plugins by `user/name`.
const LOADED_KEY: &str = "confit.plugin.loaded";
/// Registry key holding user namespace tables.
const USERS_KEY: &str = "confit.plugin.users";
/// Registry key holding scoped require results by file.
const REQUIRE_CACHE_KEY: &str = "confit.require_cache";

/// Embedded default plugin sources in external shape.
const EMBEDDED: &[(&str, &str, &str)] = &[
    (
        "solrachq",
        "mise",
        include_str!("../../plugins/solrachq/mise/plugin.lua"),
    ),
    (
        "solrachq",
        "nerd_fonts",
        include_str!("../../plugins/solrachq/nerd_fonts/plugin.lua"),
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
///
/// Missing folders read as embedded-only; the loader skips paths
/// holding no `plugin.lua`.
pub(crate) fn install(session: &crate::eval::Session) -> mlua::Result<()> {
    let lua = &session.lua;
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
    let root = session.plugins.clone();
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
fn users_index(lua: &Lua, key: Value, root: PathBuf) -> mlua::Result<Value> {
    let Some(user) = key.opt_str() else {
        return Ok(Value::Nil);
    };
    let users: Table = lua.named_registry_value(USERS_KEY)?;
    let cached: Value = users.get(user.as_str())?;
    if let Some(table) = cached.opt_table() {
        return Ok(Value::Table(table));
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
fn names_index(lua: &Lua, user: &str, key: Value, root: PathBuf) -> mlua::Result<Value> {
    let Some(name) = key.opt_str() else {
        return Ok(Value::Nil);
    };
    let loaded: Table = lua.named_registry_value(LOADED_KEY)?;
    let slot = format!("{user}/{name}");
    let stored: Value = loaded.get(slot.as_str())?;
    if !stored.is_nil()
        && (stored.as_table().is_some()
            || stored.as_function().is_some()
            || stored.as_string().is_some())
    {
        return Ok(stored);
    }
    let embedded = EMBEDDED
        .iter()
        .find(|(owner, plugin, _)| *owner == user && *plugin == name)
        .map(|(_, _, source)| *source);
    let external = root.join(user).join(&name).join("plugin.lua");
    let external = external.is_file().then_some(external);
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
    let requirer = Requirer {
        current: folder.clone(),
        folder: folder.clone(),
        scope: "plugin",
    }
    .make(lua)?;
    let env = chunk_env(lua, requirer)?;
    let chunk = lua
        .load(&source)
        .set_name(format!("@{}", file.display()))
        .set_environment(env);
    chunk
        .call(())
        .map_err(|error| plan_error(format!("confit.plugin.{user}.{name}: {error}")))
}

/// Raises one plan error with plugin attribution.
fn helpers_error_impl(message: Value) -> mlua::Result<Value> {
    const CALLER: &str = "confit.plugin.helpers.error";
    let message = message.req_str(CALLER, "message")?;
    PluginHelpers::error(CALLER, message)
}

/// Plugin helper errors holding attribution.
struct PluginHelpers;

impl PluginHelpers {
    /// Raises one plan error with plugin attribution.
    ///
    /// # Arguments
    ///
    /// * `caller` - error prefix naming the constructor.
    /// * `message` - message under reporting.
    ///
    /// # Returns
    ///
    /// Never returns a value.
    ///
    /// # Errors
    ///
    /// Always fails as a plan error carrying the message.
    ///
    fn error(caller: &str, message: String) -> mlua::Result<Value> {
        Err(plan_error(format!("{caller}: {message}")))
    }
}
