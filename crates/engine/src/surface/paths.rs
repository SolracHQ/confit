//! Paths
//!
//! Home plus config plus data plus confroot joins.

use std::path::{Path, PathBuf};

use mlua::{Lua, MultiValue, Table, Value};

use super::confit_table;
use crate::values::plan_error;

/// Installs the path namespace on a state.
pub(crate) fn install(lua: &Lua, root: &Path) -> mlua::Result<()> {
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
    let helper = lua.create_function(move |_, args: MultiValue| {
        let mut out = base.clone();
        for (position, value) in args.into_iter().enumerate() {
            match value {
                Value::String(text) => {
                    out.push(text.to_string_lossy());
                }
                _ => {
                    let index = position + 1;
                    return Err(plan_error(format!(
                        "confit.path.{name}: segment [{index}] must be a string"
                    )));
                }
            }
        }
        Ok::<String, mlua::Error>(out.to_string_lossy().into_owned())
    })?;
    namespace.set(name, helper)?;
    Ok(())
}
