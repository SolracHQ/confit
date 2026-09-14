//! Framework
//!
//! This module holds the Lua mini-stdlib. The Lua surface stays curated and
//! versioned in one place, apart from the machinery that runs it.

use std::path::Path;

use mlua::{Lua, Table};

use crate::error::{Error, Result};

// Builds a Lua domain error from format arguments.
#[macro_export]
macro_rules! lua_err {
    ($($arg:tt)*) => {
        mlua::Error::external($crate::error::Error::Lua(format!($($arg)*)))
    };
}

// Builds a plan domain error from format arguments.
#[macro_export]
macro_rules! plan_err {
    ($($arg:tt)*) => {
        mlua::Error::external($crate::error::Error::Plan(format!($($arg)*)))
    };
}

pub mod apply;
pub mod config;
pub mod document;
pub mod patch;
pub mod path;
pub mod plugin;
pub mod resources;
pub mod shell;
pub mod text;

pub use config::ConfigBuilder;
pub use patch::LuaPatch;
pub use resources::PROJECT_ROOT_KEY;

/// Fetches the confit global table for namespace setup.
///
/// Creates plus stores the table while absent, so standalone namespace
/// installs work outside `install_confit`.
///
/// # Arguments
///
/// * `lua` - state holding the global.
///
/// # Returns
///
/// The confit table receiving namespaces.
///
/// # Errors
///
/// Fails with mlua errors for table creation failures.
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

/// Installs the confit global plus every namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the global.
/// * `root` - project root for root-relative namespaces.
///
/// # Errors
///
/// Fails with Lua errors for table creation failures and with the path
/// namespace errors for missing home directories.
/// Installs the confit global plus every namespace on a Lua state.
///
/// # Arguments
///
/// * `lua` - state receiving the global.
/// * `root` - project root for root-relative namespaces.
/// * `plugins` - external plugins folder, empty keeps embedded defaults only.
///
/// # Errors
///
/// Fails with Lua errors for table creation failures and with the path
/// namespace errors for missing home directories.
pub fn install_confit(lua: &Lua, root: &Path, plugins: Option<&Path>) -> Result<()> {
    lua.globals()
        .set("confit", lua.create_table().map_err(wrap_install)?)
        .map_err(wrap_install)?;
    config::install(lua).map_err(wrap_install)?;
    resources::install(lua, root.to_path_buf()).map_err(wrap_install)?;
    document::install(lua).map_err(wrap_install)?;
    patch::install(lua).map_err(wrap_install)?;
    shell::install(lua).map_err(wrap_install)?;
    text::install(lua).map_err(wrap_install)?;
    plugin::install(lua, plugins.map(Path::to_path_buf)).map_err(wrap_install)?;
    path::install(lua, root.to_path_buf())
}

/// Maps an install-time mlua failure onto the crate error.
///
/// Install steps build tables without running Lua callbacks, so no
/// domain-carrying external errors arise here.
fn wrap_install(err: mlua::Error) -> Error {
    Error::Lua(err.to_string())
}
