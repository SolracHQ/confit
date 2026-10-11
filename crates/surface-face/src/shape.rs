//! Shape
//! Lua tables validated inside function bodies.

use std::marker::PhantomData;

use mlua::{FromLua, Lua, Table, Value};
use mlua_extras::typed::{Type, Typed};

/// One Lua table validated inside the function body.
///
/// Face types show the face and tables hold the field reads.
pub struct Shape<Face>(Table, PhantomData<Face>);

impl<Face> Clone for Shape<Face> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<Face> std::fmt::Debug for Shape<Face> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Shape").field(&self.0).finish()
    }
}

impl<Face> Shape<Face> {
    /// Return the stored table reference.
    pub fn as_table(&self) -> &Table {
        &self.0
    }

    /// Return the stored table.
    pub fn into_inner(self) -> Table {
        self.0
    }
}

impl<Face: Typed> Typed for Shape<Face> {
    fn ty() -> Type {
        Face::ty()
    }
}

impl<Face: Typed> FromLua for Shape<Face> {
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
