//! Mise
//!
//! Mise package specs for Lua.

use mlua::{Lua, Table, Value};

use super::{boundary_error, confit_table};

/// Mise package spec from `confit.mise.package`.
///
/// `name` is the package name; `version` is always populated, defaulting to `"latest"` when the
/// Lua table omits it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseSpec {
    /// Package name, e.g. `"bat"`.
    pub name: String,
    /// Pinned version or `"latest"`.
    pub version: String,
}

/// Parses a mise package spec table.
///
/// # Arguments
///
/// * `table` - spec table.
///
/// # Returns
///
/// Mise spec holding name plus version, with latest for absent version fields.
///
/// # Errors
///
/// Fails with a boundary error for fields holding values of other shapes.
pub(crate) fn parse_mise_spec(table: &Table) -> mlua::Result<MiseSpec> {
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

/// Builds a mise package table for `confit.mise.package`.
///
/// # Arguments
///
/// * `lua` - Lua state holding the table.
/// * `spec` - raw spec value.
///
/// # Returns
///
/// Table holding name plus version.
///
/// # Errors
///
/// Fails with boundary errors for values of other shapes.
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

/// Installs the mise namespace on a Lua state.
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
    let mise = lua.create_table()?;
    mise.set("package", lua.create_function(mise_package_callback)?)?;
    confit.set("mise", mise)?;
    Ok(())
}
