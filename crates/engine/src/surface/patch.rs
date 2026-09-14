//! Patch
//!
//! Patch handles plus priority levels for Lua.

use mlua::{Function, Lua, UserData, UserDataMethods, Value};

use super::confit_table;
use crate::level::Level;
use crate::values::plan_error;
use confit_core::document::StructuredFormat;

/// Patch handle built by the constructors.
#[derive(Clone)]
pub(crate) struct LuaPatch {
    /// Target document key: `rc` or one document path.
    pub(crate) target: String,
    /// Structured format for patch-created documents.
    pub(crate) format: Option<StructuredFormat>,
    /// Callback receiving the live wrapper.
    pub(crate) callback: Function,
    /// Merge priority for ordering.
    pub(crate) priority: Level,
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
    let name = match value {
        Value::String(text) => text.to_string_lossy().to_ascii_uppercase(),
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: field 'priority' must be one of {KNOWN}"
            )));
        }
    };
    Level::parse(&name).ok_or_else(|| {
        plan_error(format!(
            "{CTOR}: field 'priority' unknown level '{name}' (expected {KNOWN})"
        ))
    })
}

/// Installs the patch plus priority namespaces on a state.
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
    let callback = match callback {
        Value::Function(callback) => callback,
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: field 'callback' must be a function"
            )));
        }
    };
    Ok(LuaPatch {
        target: "rc".to_string(),
        format: None,
        callback,
        priority: Level::Normal,
    })
}

/// Builds one structured patch handle carrying the callback.
fn structured_impl(args: (Value, Value, Value)) -> mlua::Result<LuaPatch> {
    const CTOR: &str = "confit.patch.structured";
    const KNOWN: &str = "'json', 'toml', or 'yaml'";
    let (format_value, path_value, callback_value) = args;
    let format_name = match format_value {
        Value::String(text) => text.to_string_lossy(),
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: field 'format' must be one of {KNOWN}"
            )));
        }
    };
    let format = StructuredFormat::parse(&format_name)
        .ok_or_else(|| plan_error(format!("{CTOR}: field 'format' must be one of {KNOWN}")))?;
    let path = match path_value {
        Value::String(text) => text.to_string_lossy(),
        _ => return Err(plan_error(format!("{CTOR}: field 'path' must be a string"))),
    };
    let callback = match callback_value {
        Value::Function(callback) => callback,
        _ => {
            return Err(plan_error(format!(
                "{CTOR}: field 'callback' must be a function"
            )));
        }
    };
    Ok(LuaPatch {
        target: path,
        format: Some(format),
        callback,
        priority: Level::Normal,
    })
}
