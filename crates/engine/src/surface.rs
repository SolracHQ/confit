//! Surface
//!
//! Lua namespaces behind the confit global.

use std::path::Path;

use mlua::{Lua, Table};

pub(crate) mod config;
pub(crate) mod document;
pub(crate) mod patch;
pub(crate) mod paths;
pub(crate) mod plugin;
pub(crate) mod resources;
pub(crate) mod shell;
pub(crate) mod text;

/// Fetches the confit global table for namespace setup.
pub(crate) fn confit_table(lua: &Lua) -> mlua::Result<Table> {
    let globals = lua.globals();
    match globals.get::<Table>("confit") {
        Ok(table) => Ok(table),
        Err(_) => {
            let table = lua.create_table()?;
            globals.set("confit", table.clone())?;
            Ok(table)
        }
    }
}

/// Installs the confit global plus every namespace on a state.
pub(crate) fn install(
    lua: &Lua,
    root: &Path,
    plugins: Option<&Path>,
) -> confit_core::error::Result<()> {
    let fresh = lua.create_table().map_err(plan)?;
    lua.globals().set("confit", fresh).map_err(plan)?;
    config::install(lua).map_err(plan)?;
    document::install(lua).map_err(plan)?;
    patch::install(lua).map_err(plan)?;
    shell::install(lua).map_err(plan)?;
    paths::install(lua, root).map_err(plan)?;
    resources::install(lua, root).map_err(plan)?;
    text::install(lua).map_err(plan)?;
    plugin::install(lua, plugins).map_err(plan)?;
    Ok(())
}

/// Maps an install-time Lua failure onto a plan error.
fn plan(error: mlua::Error) -> confit_core::error::Error {
    confit_core::error::Error::Plan(error.to_string())
}
