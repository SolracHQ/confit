//! Namespace
//! Shared namespace registration.

use mlua::{Lua, Table, Value};

/// Wrap one namespace table as read-only under its dotted name.
///
/// # Errors
///
/// - [`mlua::Error`] for failed table and metatable writes.
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
