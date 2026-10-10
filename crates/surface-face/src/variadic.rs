//! Variadic
//! Trailing Lua arguments decoded one by one.

use mlua::{FromLua, FromLuaMulti, Lua, MultiValue};
use mlua_extras::typed::{Type, Typed};

/// Trailing Lua arguments for the verb call.
pub struct Variadic<Item>(pub Vec<Item>);

impl<Item> Variadic<Item> {
    /// Return the decoded items in call order.
    pub fn into_inner(self) -> Vec<Item> {
        self.0
    }
}

impl<Item: std::fmt::Debug> std::fmt::Debug for Variadic<Item> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Variadic").field(&self.0).finish()
    }
}

impl<Item: Clone> Clone for Variadic<Item> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<Item: Default> Default for Variadic<Item> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<Item: PartialEq> PartialEq for Variadic<Item> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<Item: Eq> Eq for Variadic<Item> {}

impl<Item: Typed + FromLua> Typed for Variadic<Item> {
    fn ty() -> Type {
        Item::ty()
    }

    fn is_variadic() -> bool {
        true
    }
}

impl<Item: Typed + FromLua> FromLuaMulti for Variadic<Item> {
    fn from_lua_multi(mut values: MultiValue, lua: &Lua) -> mlua::Result<Self> {
        let mut items = Vec::with_capacity(values.len());
        for value in values.drain(..) {
            items.push(Item::from_lua(value, lua)?);
        }
        Ok(Self(items))
    }
}

#[cfg(test)]
mod tests {
    use mlua::Value;

    use super::*;
    use crate::proxy::StrictString;

    #[test]
    fn face_returns_item_face_as_variadic() {
        assert_eq!(Variadic::<StrictString>::ty(), Type::string());
        assert!(Variadic::<StrictString>::is_variadic());
    }

    #[test]
    fn lua_read_collects_items_in_order() {
        let lua = Lua::new();
        let values = MultiValue::from_vec(vec![
            Value::String(lua.create_string("a").unwrap()),
            Value::String(lua.create_string("b").unwrap()),
            Value::String(lua.create_string("c").unwrap()),
        ]);
        let found = Variadic::<StrictString>::from_lua_multi(values, &lua).unwrap();
        assert_eq!(
            found.into_inner(),
            vec![
                StrictString("a".to_owned()),
                StrictString("b".to_owned()),
                StrictString("c".to_owned()),
            ]
        );
    }

    #[test]
    fn lua_read_rejects_mixed_items() {
        let lua = Lua::new();
        let values = MultiValue::from_vec(vec![
            Value::String(lua.create_string("a").unwrap()),
            Value::Integer(1),
        ]);
        match Variadic::<StrictString>::from_lua_multi(values, &lua) {
            Ok(_) => panic!("mixed items pass"),
            Err(error) => assert!(error.to_string().contains("integer")),
        }
    }
}
