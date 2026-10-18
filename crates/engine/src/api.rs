//! Api
//! Module installs over ready values.

pub mod config;
pub mod fact;
pub mod fetch;
pub mod open;
pub mod patch;
pub mod profile;
pub mod template;
pub mod validate;

use mlua::{Lua, Value};

use confit_store::Stores;

use crate::api::fact::Fact;
use crate::api::fetch::Fetch;
use crate::api::open::Open;
use crate::api::template::Template;
use crate::api::validate::Validate;
use crate::error::EngineError;

/// Build one builtin module value.
///
/// # Errors
///
/// - [`EngineError::UnknownModule`] for unknown names.
/// - [`mlua::Error`] for module build failures.
pub fn builtin(lua: &Lua, name: &str) -> mlua::Result<Value> {
    match name {
        Fetch::NAMESPACE => Fetch::register(lua),
        Open::NAMESPACE => Open::register(lua),
        Validate::NAMESPACE => Validate::register(lua),
        Fact::NAMESPACE => Fact::register(lua),
        Template::NAMESPACE => Template::register(lua),
        _ => Err(EngineError::UnknownModule {
            name: name.to_owned(),
        }
        .into()),
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
