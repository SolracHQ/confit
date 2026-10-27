//! Module installs over ready values.

pub mod config;
pub mod fact;
pub mod fetch;
pub mod global;
pub mod open;
pub mod patch;
pub mod profile;
pub mod template;
pub mod validate;

use mlua::{Lua, Value};

use confit_core::error::{Error, FilesystemError, Outcome};
use confit_store::Stores;

use crate::api::fact::Fact;
use crate::api::fetch::Fetch;
use crate::api::open::Open;
use crate::api::template::Template;
use crate::api::validate::Validate;

/// Build one builtin module value.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unknown names and module build failures.
/// - [`confit_core::error::kind::ErrorKind::OpenModule`] for unknown names and module build failures.
pub fn builtin(lua: &Lua, name: &str) -> Outcome<Value> {
    match name {
        Fetch::NAMESPACE => {
            Fetch::register(lua).map_err(|error| Error::lua_failed(error.to_string()))
        }
        Open::NAMESPACE => {
            Open::register(lua).map_err(|error| Error::lua_failed(error.to_string()))
        }
        Validate::NAMESPACE => {
            Validate::register(lua).map_err(|error| Error::lua_failed(error.to_string()))
        }
        Fact::NAMESPACE => {
            Fact::register(lua).map_err(|error| Error::lua_failed(error.to_string()))
        }
        Template::NAMESPACE => {
            Template::register(lua).map_err(|error| Error::lua_failed(error.to_string()))
        }
        _ => Err(Error::open_module(
            name,
            confit_core::error::ErrorContext::from(FilesystemError::FileMissing),
        )),
    }
}

/// Read the shared stores from one Lua instance.
///
/// # Errors
///
/// - [`mlua::Error`] for an uninstalled api.
pub(crate) fn stores_of(lua: &Lua) -> mlua::Result<Stores> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.clone()),
        None => Err(mlua::Error::RuntimeError(
            "confit api is not installed".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_modules_fail() {
        let lua = Lua::new();
        assert!(
            builtin(&lua, "confit.nope").is_err(),
            "unknown module fails"
        );
    }
}
