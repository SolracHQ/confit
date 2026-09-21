//! Surface
//!
//! Lua namespaces behind the confit global.

use mlua::{Lua, Table};

pub(crate) mod config;
pub(crate) mod document;
pub(crate) mod hook;
pub(crate) mod patch;
pub(crate) mod paths;
pub(crate) mod plugin;
pub(crate) mod resources;
pub(crate) mod runtime;
pub(crate) mod utils;

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

/// Installs the confit global and every namespace on a session.
pub(crate) fn install(session: &crate::eval::Session) -> confit_core::error::Result<()> {
    let lua = &session.lua;
    let fresh = lua.create_table().map_err(plan)?;
    lua.globals().set("confit", fresh).map_err(plan)?;
    config::install(lua).map_err(plan)?;
    document::install(session).map_err(plan)?;
    hook::install(lua).map_err(plan)?;
    patch::install(lua).map_err(plan)?;
    runtime::install(lua).map_err(plan)?;
    paths::install(session).map_err(plan)?;
    resources::install(session).map_err(plan)?;
    utils::install(lua).map_err(plan)?;
    plugin::install(session).map_err(plan)?;
    Ok(())
}

/// Maps an install-time Lua failure onto a plan error.
fn plan(error: mlua::Error) -> confit_core::error::Error {
    crate::error::plan(error.to_string())
}
