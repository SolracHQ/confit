//! Mismatch rendering
//!
//! Lua type words naming the received shape.

use mlua::Value;

/// Reads the Lua type word for one value.
pub fn received(value: &Value) -> &'static str {
    match value {
        Value::Nil => "nil",
        Value::Boolean(_) => "boolean",
        Value::LightUserData(_) => "userdata",
        Value::Integer(_) | Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Table(_) => "table",
        Value::Function(_) => "function",
        Value::Thread(_) => "thread",
        Value::UserData(_) => "userdata",
        Value::Error(_) => "error",
        Value::Other(_) => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::Lua;

    #[test]
    fn received_names_lua_type_words() {
        let lua = Lua::new();
        let table = Value::Table(lua.create_table().unwrap());
        let function = Value::Function(lua.create_function(|_, ()| Ok(())).unwrap());
        let cases = [
            (Value::Nil, "nil"),
            (Value::Boolean(true), "boolean"),
            (Value::Integer(1), "number"),
            (Value::Number(1.5), "number"),
            (Value::String(lua.create_string("x").unwrap()), "string"),
            (table, "table"),
            (function, "function"),
        ];
        for (value, want) in cases {
            assert_eq!(received(&value), want);
        }
    }
}
