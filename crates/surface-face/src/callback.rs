//! Callback
//! Lua callbacks over one face.

use std::marker::PhantomData;

use mlua::{FromLua, Function, Lua, Value};
use mlua_extras::typed::{Type, Typed};

/// One Lua callback over its face.
///
/// Face shows the shape and functions take the calls.
pub struct Callback<Face>(Function, PhantomData<Face>);

impl<Face> Clone for Callback<Face> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<Face> std::fmt::Debug for Callback<Face> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Callback").field(&self.0).finish()
    }
}

impl<Face> Callback<Face> {
    /// Wrap one function as its callback.
    pub fn new(function: Function) -> Self {
        Self(function, PhantomData)
    }

    /// Return the stored function reference.
    pub fn as_function(&self) -> &Function {
        &self.0
    }

    /// Return the stored function.
    pub fn into_inner(self) -> Function {
        self.0
    }
}

impl<Face: Typed> Typed for Callback<Face> {
    fn ty() -> Type {
        Face::ty()
    }
}

impl<Face> FromLua for Callback<Face> {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::Function(found) => Ok(Self(found, PhantomData)),
            _ => Err(mlua::Error::FromLuaConversionError {
                from: value.type_name(),
                to: "function".to_owned(),
                message: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Sample;

    impl Typed for Sample {
        fn ty() -> Type {
            Type::string()
        }
    }

    #[test]
    fn face_returns_marker_face() {
        assert_eq!(Callback::<Sample>::ty(), Sample::ty());
    }

    #[test]
    fn lua_read_accepts_functions() {
        let lua = Lua::new();
        let func = lua.create_function(|_, value: String| Ok(value)).unwrap();
        let callback = Callback::<Sample>::from_lua(Value::Function(func), &lua).unwrap();
        let back: String = callback.as_function().call("hi").unwrap();
        assert_eq!(back, "hi");
    }

    #[test]
    fn lua_read_rejects_non_functions() {
        let lua = Lua::new();
        let cases = vec![
            Value::String(lua.create_string("hi").unwrap()),
            Value::Integer(1),
        ];
        for value in cases {
            match Callback::<Sample>::from_lua(value, &lua) {
                Ok(_) => panic!("non-function passes"),
                Err(mlua::Error::FromLuaConversionError { to, .. }) => {
                    assert_eq!(to, "function");
                }
                Err(error) => panic!("wrong error: {error}"),
            }
        }
    }
}
