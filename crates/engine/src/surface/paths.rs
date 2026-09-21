//! Paths
//!
//! Home, config, data, and confroot joins.

use std::path::{Path, PathBuf};

use mlua::{Lua, MultiValue, Table};

use super::confit_table;
use crate::error::plan_error;
use crate::lua::ValueExt;

/// Installs the path namespace on a state.
pub(crate) fn install(session: &crate::eval::Session) -> mlua::Result<()> {
    let lua = &session.lua;
    let root = session.root.clone();
    let bases = directories::BaseDirs::new().ok_or_else(|| {
        plan_error(
            "confit.path: cannot resolve home directory: BaseDirs::new returned None; \
             set HOME to a valid directory",
        )
    })?;
    let namespace = lua.create_table()?;
    register(lua, &namespace, "home", bases.home_dir().to_path_buf())?;
    register(lua, &namespace, "config", bases.config_dir().to_path_buf())?;
    register(lua, &namespace, "data", bases.data_dir().to_path_buf())?;
    register(lua, &namespace, "confroot", root.to_path_buf())?;
    confit_table(lua)?.set("path", namespace)?;
    Ok(())
}

/// Registers one join helper under a base folder.
fn register(lua: &Lua, namespace: &Table, name: &'static str, base: PathBuf) -> mlua::Result<()> {
    let helper = lua.create_function(move |_, args: MultiValue| join_impl(name, &base, args))?;
    namespace.set(name, helper)?;
    Ok(())
}

/// Joins path segments onto one base folder.
///
/// # Arguments
///
/// * `name` - helper name naming the base.
/// * `base` - base folder under joining.
/// * `args` - segment values in call order.
///
/// # Returns
///
/// Joined path as a string.
///
/// # Errors
///
/// Non-string segments fail as plan errors.
///
fn join_impl(name: &str, base: &Path, args: MultiValue) -> mlua::Result<String> {
    let mut out = base.to_path_buf();
    for (position, value) in args.into_iter().enumerate() {
        let index = position + 1;
        let segment = value.req_str(
            &format!("confit.path.{name}"),
            &format!("segment [{index}]"),
        )?;
        out.push(segment);
    }
    Ok(out.to_string_lossy().into_owned())
}
