//! Patch
//!
//! Patch handles and priority levels for Lua.

use mlua::{Function, Lua, UserData, UserDataMethods, Value};

use super::confit_table;
use crate::error::plan_error;
use crate::level::Level;
use crate::lua::ValueExt;
use confit_model::document::StructuredFormat;

/// Patch handle built by the constructors.
#[derive(Clone)]
pub(crate) struct LuaPatch {
    /// Target document key: `rc` or one destination display.
    pub(crate) target: String,
    /// Structured format for patch-created documents.
    pub(crate) format: Option<StructuredFormat>,
    /// Callback receiving the live wrapper.
    pub(crate) callback: Function,
    /// Merge priority for ordering.
    pub(crate) priority: Level,
}

impl LuaPatch {
    /// Builds one rc patch handle carrying the callback.
    ///
    /// # Arguments
    ///
    /// * `callback` - callback receiving the live rc wrapper.
    ///
    /// # Returns
    ///
    /// Patch handle targeting the rc document at normal priority.
    ///
    fn rc(callback: Function) -> Self {
        Self {
            target: "rc".to_string(),
            format: None,
            callback,
            priority: Level::Normal,
        }
    }

    /// Builds one structured patch handle carrying the callback.
    ///
    /// # Arguments
    ///
    /// * `format_name` - raw format name under parsing.
    /// * `target` - target destination display.
    /// * `callback` - callback receiving the live wrapper.
    ///
    /// # Returns
    ///
    /// Patch handle targeting the destination at normal priority.
    ///
    /// # Errors
    ///
    /// Unknown format names fail as plan errors.
    ///
    fn structured(format_name: String, target: String, callback: Function) -> mlua::Result<Self> {
        const CTOR: &str = "confit.patch.structured";
        const KNOWN: &str = "'json', 'toml', or 'yaml'";
        let format = StructuredFormat::parse(&format_name)
            .ok_or_else(|| plan_error(format!("{CTOR}: field 'format' must be one of {KNOWN}")))?;
        Ok(Self {
            target,
            format: Some(format),
            callback,
            priority: Level::Normal,
        })
    }
}

impl UserData for LuaPatch {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("priority", |_, this, level: Value| {
            this.priority = parse_level(level)?;
            Ok(this.clone())
        });
    }
}

/// Parses one priority level from a Lua value.
fn parse_level(value: Value) -> mlua::Result<Level> {
    const CTOR: &str = "confit.patch";
    const KNOWN: &str = "'MINOR', 'LOW', 'NORMAL', 'HIGH', or 'MAJOR'";
    let raw = value.req_str(CTOR, "priority")?;
    let name = raw.to_ascii_uppercase();
    Level::parse(&name).ok_or_else(|| {
        plan_error(format!(
            "{CTOR}: field 'priority' unknown level '{name}' (expected {KNOWN})"
        ))
    })
}

/// Installs the patch and priority namespaces on a state.
pub(crate) fn install(lua: &Lua) -> mlua::Result<()> {
    let confit = confit_table(lua)?;
    let namespace = lua.create_table()?;
    namespace.set(
        "rc",
        lua.create_function(|_, callback: Value| rc_impl(callback))?,
    )?;
    namespace.set(
        "structured",
        lua.create_function(|_, args: (Value, Value, Value)| structured_impl(args))?,
    )?;
    confit.set("patch", namespace)?;
    let priority = lua.create_table()?;
    for name in ["MINOR", "LOW", "NORMAL", "HIGH", "MAJOR"] {
        priority.set(name, name)?;
    }
    confit.set("priority", priority)?;
    Ok(())
}

/// Builds one rc patch handle carrying the callback.
fn rc_impl(callback: Value) -> mlua::Result<LuaPatch> {
    const CTOR: &str = "confit.patch.rc";
    let callback = callback.req_func(CTOR, "callback")?;
    Ok(LuaPatch::rc(callback))
}

/// Builds one structured patch handle carrying the callback.
fn structured_impl(args: (Value, Value, Value)) -> mlua::Result<LuaPatch> {
    const CTOR: &str = "confit.patch.structured";
    let (format_value, path_value, callback_value) = args;
    let format_name = format_value.req_str(CTOR, "format")?;
    let target = super::handles::req_target_path(&path_value, CTOR, "path")?;
    let callback = callback_value.req_func(CTOR, "callback")?;
    LuaPatch::structured(format_name, target, callback)
}
