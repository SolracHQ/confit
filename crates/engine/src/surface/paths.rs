//! Paths
//!
//! Destination handles over XDG bases plus literals.

use mlua::{Lua, MultiValue, Table};

use super::confit_table;
use super::handles::LuaRoute;
use crate::error::plan_error;
use crate::lua::ValueExt;
use confit_model::handles::{Route, RouteBase};

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
/// # Arguments
///
/// * `lua` - state owning the route userdata.
/// * `name` - helper name naming the base.
/// * `base` - destination base under joining.
/// * `args` - segment values in call order.
///
/// # Returns
///
/// Route userdata carrying the base plus the joined path.
///
/// # Errors
///
/// Missing segments fail as plan errors. Non-string
/// segments fail as plan errors. Empty routes fail as
/// plan errors.
///
fn base_impl(_lua: &Lua, name: &str, base: RouteBase, args: MultiValue) -> mlua::Result<LuaRoute> {
    let caller = format!("confit.path.{name}");
    let mut segments = Vec::new();
    for (position, value) in args.into_iter().enumerate() {
        let index = position + 1;
        let segment = value.req_str(&caller, &format!("segment [{index}]"))?;
        segments.push(segment);
    }
    if segments.is_empty() {
        return Err(plan_error(format!(
            "{caller}: field 'segments' must hold one path at least"
        )));
    }
    let relative = segments.join("/");
    Route::new(base, relative)
        .map(LuaRoute::from)
        .map_err(|error| plan_error(format!("{caller}: {error}")))
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
