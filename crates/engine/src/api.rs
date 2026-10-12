//! Api
//! Module installs over ready values.

pub mod config;
pub mod fact;
pub mod fetch;
pub mod patch;
pub mod profile;
pub mod resource;
pub mod template;
pub mod validate;

use mlua::{Lua, Value};

use confit_store::Stores;

use crate::error::EngineError;

/// Build one builtin module value.
///
/// # Errors
///
/// - [`EngineError::UnknownModule`] for unknown names.
/// - [`mlua::Error`] for module build failures.
pub fn builtin(lua: &Lua, name: &str) -> mlua::Result<Value> {
    match name {
        crate::api::fetch::Module::NAMESPACE => crate::api::fetch::Module::register(lua),
        crate::api::resource::Module::NAMESPACE => crate::api::resource::Module::register(lua),
        crate::api::validate::Module::NAMESPACE => crate::api::validate::Module::register(lua),
        crate::api::fact::Module::NAMESPACE => crate::api::fact::Module::register(lua),
        crate::api::template::Module::NAMESPACE => crate::api::template::Module::register(lua),
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
        match builtin(&lua, "confit.nope") {
            Ok(_) => panic!("unknown module passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("unknown module"),
                    "the text shows the miss: {text}"
                );
            }
        }
    }
}
