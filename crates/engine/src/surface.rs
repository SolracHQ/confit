//! Surface
//!
//! Lua namespaces behind the confit global.

use mlua::{Lua, Table};

pub(crate) mod config;
pub(crate) mod document;
pub(crate) mod handles;
pub(crate) mod hook;
pub(crate) mod patch;
pub(crate) mod paths;
pub(crate) mod plugin;
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
///
/// # Errors
///
/// - [`crate::error::EngineError::Unknown`] for install-time
///   Lua failures.
pub(crate) fn install(session: &crate::eval::Session) -> crate::error::Result<()> {
    let lua = &session.lua;
    let fresh = lua.create_table()?;
    lua.globals().set("confit", fresh)?;
    config::install(lua)?;
    document::install(session)?;
    handles::install(session)?;
    hook::install(lua)?;
    patch::install(lua)?;
    runtime::install(lua)?;
    paths::install(lua)?;
    utils::install(lua)?;
    plugin::install(session)?;
    Ok(())
}
