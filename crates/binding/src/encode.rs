//! Encode
//! Blanket encoding for container and foreign faces.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use mlua::{Function, Lua, Value};

use crate::value::Encode;

/// Encode one boxed face through its inner face.
impl<T: Encode> Encode for Box<T> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        self.as_ref().encode(lua)
    }
}

/// Encode one hash map through encoded entries.
impl<K: Encode, V: Encode> Encode for HashMap<K, V> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        for (key, item) in self {
            table.set(key.encode(lua)?, item.encode(lua)?)?;
        }
        Ok(Value::Table(table))
    }
}

/// Encode one sorted map through encoded entries.
impl<K: Encode, V: Encode> Encode for BTreeMap<K, V> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        let table = lua.create_table()?;
        for (key, item) in self {
            table.set(key.encode(lua)?, item.encode(lua)?)?;
        }
        Ok(Value::Table(table))
    }
}

/// Encode one JSON value through serde values.
impl Encode for serde_json::Value {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        use mlua::LuaSerdeExt as _;
        lua.to_value(self)
    }
}

/// Encode one path through its display string.
impl Encode for PathBuf {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        lua.create_string(self.to_string_lossy().as_ref())
            .map(Value::String)
    }
}

/// Encode one function as itself.
impl Encode for Function {
    fn encode(&self, _lua: &Lua) -> mlua::Result<Value> {
        Ok(Value::Function(self.clone()))
    }
}

/// Encode one machine word through integers.
impl Encode for usize {
    fn encode(&self, _lua: &Lua) -> mlua::Result<Value> {
        Ok(Value::Integer(*self as i64))
    }
}

/// Encode one signed word through integers.
impl Encode for isize {
    fn encode(&self, _lua: &Lua) -> mlua::Result<Value> {
        Ok(Value::Integer(*self as i64))
    }
}

/// Encode one hash through its hex digest.
impl Encode for confit_driver::sha::Sha {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        lua.create_string(self.hex().as_str()).map(Value::String)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Decode;

    #[test]
    fn boxed_values_encode_inner_faces() {
        let lua = Lua::new();
        let boxed = Box::new("held".to_owned());
        let encoded = boxed.encode(&lua).unwrap();
        assert!(matches!(encoded, Value::String(found) if found.to_string_lossy() == "held"));
    }

    #[test]
    fn maps_encode_entries_with_decode_round_trip() {
        let lua = Lua::new();
        let mut source = BTreeMap::new();
        source.insert("alpha".to_owned(), "one".to_owned());
        source.insert("beta".to_owned(), "two".to_owned());
        let encoded = source.encode(&lua).unwrap();
        let back = BTreeMap::<String, String>::decode(&encoded, &lua).unwrap();
        assert_eq!(back, source);
        let mut hashed = HashMap::new();
        hashed.insert("alpha".to_owned(), 1u32);
        let encoded = hashed.encode(&lua).unwrap();
        let Value::Table(table) = encoded else {
            panic!("map encodes a table");
        };
        let found: u32 = table.get("alpha").unwrap();
        assert_eq!(found, 1);
    }

    #[test]
    fn json_objects_encode_tables_with_decode_round_trip() {
        let lua = Lua::new();
        let source = serde_json::json!({"alpha": "one", "count": 2});
        let encoded = source.encode(&lua).unwrap();
        let back = serde_json::Value::decode(&encoded, &lua).unwrap();
        assert_eq!(back, source);
    }

    #[test]
    fn paths_encode_display_strings() {
        let lua = Lua::new();
        let route = PathBuf::from("home/bin");
        let encoded = route.encode(&lua).unwrap();
        assert!(matches!(encoded, Value::String(found) if found.to_string_lossy() == "home/bin"));
    }

    #[test]
    fn functions_and_words_encode_plain_shapes() {
        let lua = Lua::new();
        let function: Function = lua.load("return function() end").call(()).unwrap();
        let encoded = function.encode(&lua).unwrap();
        assert!(matches!(encoded, Value::Function(_)));
        assert!(matches!(3usize.encode(&lua).unwrap(), Value::Integer(3)));
        assert!(matches!(
            (-2isize).encode(&lua).unwrap(),
            Value::Integer(-2)
        ));
    }

    #[test]
    fn hashes_encode_hex_digests() {
        let lua = Lua::new();
        let digest = confit_driver::sha::Sha::hash(b"confit fixture");
        let encoded = digest.encode(&lua).unwrap();
        assert!(matches!(encoded, Value::String(found) if found.to_string_lossy() == digest.hex()));
    }
}
