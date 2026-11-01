//! Module tables exposed to Lua with read-only views.
//!
//! A `lua_module` block exposes its statics under a dotted path.

use mlua::{Lua, Table, Value};

/// Wraps a module table as read-only under its dotted path.
///
/// Lua writes raise a runtime error with the module path.
///
/// # Errors
///
/// - [`mlua::Error`] for a failed table or metatable write.
pub fn readonly(lua: &Lua, name: &'static str, inner: Table) -> mlua::Result<Value> {
    let proxy = lua.create_table()?;
    let meta = lua.create_table()?;
    meta.set("__index", inner)?;
    let message = format!("cannot write '{name}': read-only table");
    let reject = lua.create_function(move |_, _: (Table, Value, Value)| {
        Err::<(), _>(mlua::Error::RuntimeError(message.clone()))
    })?;
    meta.set("__newindex", reject)?;
    meta.set("__metatable", false)?;
    proxy.set_metatable(Some(meta))?;
    Ok(Value::Table(proxy))
}
