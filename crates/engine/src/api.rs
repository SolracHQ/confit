//! Installs the builtin modules.

pub mod archive;
pub mod config;
pub mod fact;
pub mod fetch;
pub mod global;
pub mod input;
pub mod open;
pub mod patch;
pub mod profile;
pub mod render;
pub mod resource;
pub mod validate;

use mlua::{Lua, Value};

use confit_core::error::{
    ContextualCause, FilesystemError, Terminal, TerminalResult, contextual, error, kind::ErrorKind,
};
use confit_store::Stores;

use crate::api::fact::Fact;
use crate::api::fetch::Fetch;
use crate::api::input::Input;
use crate::api::open::Open;
use crate::api::render::Render;
use crate::api::validate::Validate;

/// Builds the named builtin module.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a module fails
///   to build.
/// - [`confit_core::error::kind::ErrorKind::OpenModule`] when the name is
///   unknown.
pub fn builtin(lua: &Lua, name: &str) -> TerminalResult<Value> {
    match name {
        Fetch::MODULE => Fetch::add(lua)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed),
        Open::MODULE => Open::add(lua)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed),
        Validate::MODULE => Validate::add(lua)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed),
        Fact::MODULE => Fact::add(lua)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed),
        Input::MODULE => Input::add(lua)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed),
        Render::MODULE => Render::add(lua)
            .map_err(ContextualCause::from)
            .terminal(ErrorKind::LuaFailed),
        _ => error(
            ErrorKind::OpenModule,
            contextual(FilesystemError::FileMissing).with_module(name),
        ),
    }
}

/// Returns the api stores.
///
/// # Errors
///
/// - [`mlua::Error`] when the api is not installed.
pub(crate) fn stores_from(lua: &Lua) -> mlua::Result<Stores> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.clone()),
        None => Err(mlua::Error::RuntimeError(
            "the confit api is not installed".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_rejects_unknown_names() {
        let lua = Lua::new();
        assert!(
            builtin(&lua, "confit.nope").is_err(),
            "the builtin resolver rejects an unknown name"
        );
    }
}
