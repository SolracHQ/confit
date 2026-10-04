//! Callback
//! Lua callbacks over one live document type.

use std::marker::PhantomData;

use mlua::{FromLua, Function, Lua, Value};
use mlua_extras::typed::{Param, Type, Typed};

use crate::module::Named;

/// One Lua callback over its live document type.
///
/// The document type names the face, the function takes calls.
pub struct Callback<Document>(Function, PhantomData<Document>);

impl<Document> Clone for Callback<Document> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<Document> std::fmt::Debug for Callback<Document> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Callback").field(&self.0).finish()
    }
}

impl<Document> Callback<Document> {
    /// Wraps one function as its callback.
    pub fn new(function: Function) -> Self {
        Self(function, PhantomData)
    }

    /// Reads the stored function reference.
    pub fn as_function(&self) -> &Function {
        &self.0
    }

    /// Returns the stored function.
    pub fn into_inner(self) -> Function {
        self.0
    }
}

impl<Document: Typed> Typed for Callback<Document> {
    fn ty() -> Type {
        Type::Function {
            params: vec![Param {
                name: Some("document".into()),
                doc: None,
                ty: Named::<Document>::ty(),
            }],
            returns: vec![],
        }
    }
}

impl<Document: Typed> FromLua for Callback<Document> {
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

    struct Doc;

    impl Typed for Doc {
        fn ty() -> Type {
            Type::string()
        }
    }

    #[test]
    fn face_names_one_document_param_without_returns() {
        assert_eq!(
            Callback::<Doc>::ty(),
            Type::Function {
                params: vec![Param {
                    name: Some("document".into()),
                    doc: None,
                    ty: Named::<Doc>::ty(),
                }],
                returns: vec![],
            }
        );
    }

    #[test]
    fn lua_read_accepts_functions() {
        let lua = Lua::new();
        let func = lua.create_function(|_, value: String| Ok(value)).unwrap();
        let callback = Callback::<Doc>::from_lua(Value::Function(func), &lua).unwrap();
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
            match Callback::<Doc>::from_lua(value, &lua) {
                Ok(_) => panic!("non-function passes"),
                Err(mlua::Error::FromLuaConversionError { to, .. }) => {
                    assert_eq!(to, "function");
                }
                Err(error) => panic!("wrong error: {error}"),
            }
        }
    }
}
