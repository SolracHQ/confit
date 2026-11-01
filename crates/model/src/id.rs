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
    /// Returns whether both identities hold the same string.
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for Id {}

impl Serialize for Id {
    /// Writes the identity string with no trace.
    fn serialize<Encoder>(&self, encoder: Encoder) -> Result<Encoder::Ok, Encoder::Error>
    where
        Encoder: Serializer,
    {
        encoder.serialize_str(self.id.as_str())
    }
}

impl<'de> Deserialize<'de> for Id {
    /// Decodes a bare string into an identity with an empty trace.
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
    /// Returns an identity with the given string and an empty trace.
    pub fn of(name: impl Into<String>) -> Self {
        Self {
            id: name.into(),
            ..Default::default()
        }
    }

    /// Returns the string that the identity holds.
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

    /// A second userdata shape for decode tests.
    #[allow(dead_code)]
    #[derive(Clone, Shape)]
    #[shape(object)]
    struct OtherShape {
        seed: String,
    }

    #[lua_methods]
    impl OtherShape {}

    #[test]
    fn id_decode_rejects_text_tables_and_other_userdata() {
        let lua = Lua::new();
        let text = lua.create_string("same").unwrap();
        assert!(
            Id::decode(&Value::String(text), &lua).is_err(),
            "the identity decoder rejects a string"
        );
        let table = lua.create_table().unwrap();
        assert!(
            Id::decode(&Value::Table(table), &lua).is_err(),
            "the identity decoder rejects a table"
        );
        assert!(
            Id::decode(&Value::Nil, &lua).is_err(),
            "the identity decoder rejects nil"
        );
        let foreign = lua
            .create_userdata(OtherShape {
                seed: "base".to_owned(),
            })
            .unwrap();
        assert!(
            Id::decode(&Value::UserData(foreign), &lua).is_err(),
            "the identity decoder rejects other userdata"
        );
    }

    #[test]
    fn traced_id_starts_with_an_empty_trace() {
        let lua = Lua::new();
        let traced = Id::traced(&lua, Id::of("same"));
        assert_eq!(traced.as_str(), "same");
        assert!(
            traced.trace.0.is_empty(),
            "a fresh identity starts with an empty trace"
        );
    }
}
