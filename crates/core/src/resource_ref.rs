//! Content hash references.

use lua_binding::Shape;
use lua_binding::lua_methods;
use serde::{Deserialize, Serialize};

use crate::sha::Sha;

/// A reference to the data.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Shape,
)]
#[serde(transparent)]
#[shape(object)]
pub struct ResourceRef {
    sha: Sha,
}

impl ResourceRef {
    /// Wraps one content hash.
    ///
    /// - Only call with a hash the store returned.
    pub fn new(sha: Sha) -> Self {
        Self { sha }
    }

    /// Returns the content hash.
    pub fn sha(&self) -> &Sha {
        &self.sha
    }

    /// Writes the digest as lowercase hex.
    pub fn hex(&self) -> String {
        self.sha.hex()
    }
}

#[lua_methods]
impl ResourceRef {}

#[cfg(test)]
mod tests {
    use super::*;
    use lua_binding::value::{Decode, Encode};
    use mlua::{Lua, Value};

    #[test]
    fn resource_ref_round_trips_hex() {
        let sha = Sha::hash(b"reference fixture");
        let found = ResourceRef::new(sha);
        let text = serde_json::to_string(&found).unwrap();
        assert_eq!(text, format!("\"{}\"", sha.hex()));
        match serde_json::from_str::<ResourceRef>(&text) {
            Ok(back) => assert_eq!(back, found),
            Err(error) => panic!("the reference fails to decode: {error}"),
        }
    }

    #[test]
    fn resource_ref_rejects_a_malformed_digest() {
        assert!(serde_json::from_str::<ResourceRef>("\"short\"").is_err());
    }

    #[test]
    fn resource_ref_decode_rejects_strings() {
        let lua = Lua::new();
        let sha = Sha::hash(b"reference fixture");
        let text = Value::String(lua.create_string(sha.hex()).unwrap());
        assert!(
            ResourceRef::decode(&text, &lua).is_err(),
            "the reference decoder rejects a string"
        );
        let table = Value::Table(lua.create_table().unwrap());
        assert!(
            ResourceRef::decode(&table, &lua).is_err(),
            "the reference decoder rejects a table"
        );
        assert!(
            ResourceRef::decode(&Value::Nil, &lua).is_err(),
            "the reference decoder rejects nil"
        );
    }

    #[test]
    fn resource_ref_round_trips_userdata() {
        let lua = Lua::new();
        let found = ResourceRef::new(Sha::hash(b"reference fixture"));
        let value = found.encode(&lua).unwrap();
        match ResourceRef::decode(&value, &lua) {
            Ok(back) => assert_eq!(back, found),
            Err(error) => panic!("the userdata fails to decode: {error}"),
        }
    }
}
