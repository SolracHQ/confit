//! Framework
//!
//! This module holds the Lua mini-stdlib. The Lua surface stays curated and
//! versioned in one place, apart from the machinery that runs it.

use std::path::Path;

use mlua::{Lua, Table, Value};

use crate::error::{Error, Result};

// Builds a Lua domain error from format arguments.
macro_rules! lua_err {
    ($($arg:tt)*) => {
        mlua::Error::external($crate::error::Error::Lua(format!($($arg)*)))
    };
}

// Builds a plan domain error from format arguments.
macro_rules! plan_err {
    ($($arg:tt)*) => {
        mlua::Error::external($crate::error::Error::Plan(format!($($arg)*)))
    };
}

pub mod artifact;
pub mod mise;
pub mod path;
pub mod resources;
pub mod tool;

pub use artifact::LuaArtifact;
pub use mise::MiseSpec;
pub use resources::{MergeOpts, PROJECT_ROOT_KEY};
pub use tool::{InitEntryShape, ToolBuilder, ToolContribution};

/// Wraps a message as a Lua domain error.
///
/// # Arguments
///
/// * `message` - error text.
///
/// # Returns
///
/// Lua domain error carrying the message.
pub(crate) fn boundary_error(message: String) -> mlua::Error {
    mlua::Error::external(Error::Lua(message))
}

/// Builds a field error naming tool plus field.
///
/// # Arguments
///
/// * `tool` - tool name.
/// * `field` - Lua field.
/// * `detail` - violated expectation.
///
/// # Returns
///
/// Lua domain error naming tool plus field.
pub(crate) fn field_error(tool: &str, field: &str, detail: &str) -> mlua::Error {
    boundary_error(format!("tool '{tool}': field '{field}' {detail}"))
}

/// Reads a string from a Lua value.
///
/// # Arguments
///
/// * `value` - raw Lua value.
/// * `tool` - tool name for errors.
/// * `field` - field name for errors.
///
/// # Returns
///
/// String value.
///
/// # Errors
///
/// Fails with a field error for values of other shapes.
pub(crate) fn take_string(value: Value, tool: &str, field: &str) -> mlua::Result<String> {
    match value {
        Value::String(text) => Ok(text.to_string_lossy()),
        _ => Err(field_error(tool, field, "must be a string")),
    }
}

/// Reads a string array from a Lua table.
///
/// # Arguments
///
/// * `table` - Lua table holding the array.
/// * `tool` - tool name for errors.
/// * `field` - field name for errors.
///
/// # Returns
///
/// String values in index order.
///
/// # Errors
///
/// Fails with a field error for gaps in index order and for entries holding values of other shapes.
pub(crate) fn take_string_array(
    table: &Table,
    tool: &str,
    field: &str,
) -> mlua::Result<Vec<String>> {
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
pub fn install_confit(lua: &Lua, root: &Path) -> Result<()> {
    lua.globals()
        .set("confit", lua.create_table().map_err(wrap_install)?)
        .map_err(wrap_install)?;
    tool::install(lua).map_err(wrap_install)?;
    mise::install(lua).map_err(wrap_install)?;
    resources::install(lua, root.to_path_buf()).map_err(wrap_install)?;
    artifact::install(lua).map_err(wrap_install)?;
    path::install(lua, root.to_path_buf())
}

/// Maps an install-time mlua failure onto the crate error.
///
/// Install steps build tables without running Lua callbacks, so no
/// domain-carrying external errors arise here.
fn wrap_install(err: mlua::Error) -> Error {
    Error::Lua(err.to_string())
}
