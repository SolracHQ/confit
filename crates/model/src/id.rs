//! Id
//! Opaque identities with declaration stacks.

use lua_binding::trace::LuaTrace;
use lua_binding::{Shape, lua_methods};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// One opaque identity.
#[derive(Clone, Debug, Default, Shape)]
#[shape(traced, object)]
pub struct Id {
    /// The full identity string.
    pub id: String,
    /// The identity declaration stack.
    #[shape(lua_trace)]
    pub trace: LuaTrace,
}

impl PartialEq for Id {
    /// Report whole string equality.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Id {}

impl Serialize for Id {
    /// Encode one id as its full string.
    fn serialize<Encoder>(&self, encoder: Encoder) -> Result<Encoder::Ok, Encoder::Error>
    where
        Encoder: Serializer,
    {
        encoder.serialize_str(self.id.as_str())
    }
}

impl<'de> Deserialize<'de> for Id {
    /// Decode one bare string into an id with an empty trace.
    fn deserialize<Reader>(reader: Reader) -> Result<Self, Reader::Error>
    where
        Reader: Deserializer<'de>,
    {
        String::deserialize(reader).map(|id| Self {
            id,
            ..Default::default()
        })
    }
}

impl Id {
    /// Build one id from its full string with an empty trace.
    pub fn of(name: impl Into<String>) -> Self {
        Self {
            id: name.into(),
            ..Default::default()
        }
    }

    /// Return the full identity string.
    pub fn as_str(&self) -> &str {
        self.id.as_str()
    }
}

#[lua_methods]
impl Id {}

#[cfg(test)]
mod tests {
    use super::*;
    use lua_binding::Shape;
    use lua_binding::value::Decode;
    use mlua::{Lua, Value};

    /// Foreign handle for id decode tests.
    #[allow(dead_code)]
    #[derive(Clone, Shape)]
    #[shape(object)]
    struct Foreign {
        seed: String,
    }

    #[lua_methods]
    impl Foreign {}

    #[test]
    fn id_decode_rejects_wrong_shapes() {
        let lua = Lua::new();
        let text = lua.create_string("same").unwrap();
        assert!(
            Id::decode(&Value::String(text), &lua).is_err(),
            "string fails"
        );
        let table = lua.create_table().unwrap();
        assert!(
            Id::decode(&Value::Table(table), &lua).is_err(),
            "table fails"
        );
        assert!(Id::decode(&Value::Nil, &lua).is_err(), "nil fails");
        let foreign = lua
            .create_userdata(Foreign {
                seed: "base".to_owned(),
            })
            .unwrap();
        assert!(
            Id::decode(&Value::UserData(foreign), &lua).is_err(),
            "wrong userdata fails"
        );
    }

    #[test]
    fn traced_outside_lua_holds_empty_trace() {
        let lua = Lua::new();
        let traced = Id::traced(&lua, Id::of("same"));
        assert_eq!(traced.as_str(), "same");
        assert!(traced.trace.0.is_empty(), "bare state holds no frames");
    }
}
