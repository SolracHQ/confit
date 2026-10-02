//! Paths
//!
//! Destination handles over XDG bases and literals.

use mlua::{Lua, MultiValue, Table};

use super::confit_table;
use super::handles::LuaRoute;
use crate::error::{EngineError, FieldRef, Scope};
use confit_model::routes::{Route, RouteBase};

/// Installs the path namespace on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let namespace = lua.create_table()?;
    register(lua, &namespace, "home", RouteBase::Home)?;
    register(lua, &namespace, "config", RouteBase::Config)?;
    register(lua, &namespace, "data", RouteBase::Data)?;
    register(lua, &namespace, "cache", RouteBase::Cache)?;
    let literal = lua.create_function(|lua, args: MultiValue| literal_impl(lua, args))?;
    namespace.set("literal", literal)?;
    confit_table(lua)?.set("path", namespace)?;
    Ok(())
}

/// Registers one route helper under a destination base.
fn register(lua: &Lua, namespace: &Table, name: &'static str, base: RouteBase) -> mlua::Result<()> {
    let helper =
        lua.create_function(move |lua, args: MultiValue| base_impl(lua, name, base, args))?;
    namespace.set(name, helper)?;
    Ok(())
}

/// Builds one destination route from segments under a base.
///
/// # Errors
///
/// - [`EngineError::Field`] for missing segments and
///   non-string segments.
/// - [`EngineError::NestScope`] for destination builds.
///
fn base_impl(
    _lua: &Lua,
    name: &'static str,
    base: RouteBase,
    args: MultiValue,
) -> mlua::Result<LuaRoute> {
    let scope = Scope::PathBase { name };
    let mut segments = Vec::new();
    for (position, value) in args.into_iter().enumerate() {
        let index = position + 1;
        let segment = match value {
            mlua::Value::String(text) => text.to_string_lossy(),
            _ => {
                return Err(EngineError::Field {
                    scope: scope.clone(),
                    field: FieldRef::index("segment", index),
                    want: "must be a string",
                }
                .into());
            }
        };
        segments.push(segment);
    }
    if segments.is_empty() {
        return Err(EngineError::Field {
            scope: scope.clone(),
            field: FieldRef::name("segments"),
            want: "must hold one path at least",
        }
        .into());
    }
    let relative = segments.join("/");
    Route::new(base, relative)
        .map(LuaRoute::from)
        .map_err(|error| {
            EngineError::NestScope {
                scope: scope.clone(),
                reason: error.to_string(),
            }
            .into()
        })
}

/// Builds one literal destination route from segments.
///
/// Literal routes carry host-specific paths verbatim.
///
/// # Arguments
///
/// * `lua` - state owning the route userdata.
/// * `args` - segment values in call order.
///
/// # Returns
///
/// Route userdata carrying the literal base.
///
/// # Errors
///
/// Missing segments fail as plan errors. Non-string
/// segments fail as plan errors. Empty routes fail as
/// plan errors.
///
fn literal_impl(lua: &Lua, args: MultiValue) -> mlua::Result<LuaRoute> {
    base_impl(lua, "literal", RouteBase::Literal, args)
}
