//! Shape
//! Lua tables validated inside verb bodies.

use std::marker::PhantomData;

use mlua::{FromLua, Lua, Table, Value};
use mlua_extras::typed::{Type, Typed};

/// One Lua table validated inside the verb body.
///
/// Data types show the face and tables hold the field reads.
pub struct Shape<Data>(Table, PhantomData<Data>);

impl<Data> Clone for Shape<Data> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<Data> std::fmt::Debug for Shape<Data> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Shape").field(&self.0).finish()
    }
}

impl<Data> Shape<Data> {
    /// Return the stored table reference.
    pub fn as_table(&self) -> &Table {
        &self.0
    }

    /// Return the stored table.
    pub fn into_inner(self) -> Table {
        self.0
    }
}

impl<Data: Typed> Typed for Shape<Data> {
    fn ty() -> Type {
        Data::ty()
    }
}

impl<Data: Typed> FromLua for Shape<Data> {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::Table(found) => Ok(Self(found, PhantomData)),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "table".to_owned(),
                message: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use schemars::JsonSchema;

    use super::*;

    #[derive(JsonSchema)]
    struct Point {
        #[allow(dead_code)]
        label: String,
    }

    crate::typed!(Point);

    #[test]
    fn face_returns_inner_face() {
        assert_eq!(Shape::<Point>::ty(), Point::ty());
    }

    #[test]
    fn lua_read_accepts_tables() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("label", "here").unwrap();
        let shape = Shape::<Point>::from_lua(Value::Table(table), &lua).unwrap();
        let back: String = shape.as_table().get("label").unwrap();
        assert_eq!(back, "here");
    }

    #[test]
    fn lua_read_rejects_non_tables() {
        let lua = Lua::new();
        let value = Value::String(lua.create_string("hi").unwrap());
        match Shape::<Point>::from_lua(value, &lua) {
            Ok(_) => panic!("string passes as table"),
            Err(mlua::Error::FromLuaConversionError { to, .. }) => {
                assert_eq!(to, "table");
            }
            Err(error) => panic!("wrong error: {error}"),
        }
    }
}
