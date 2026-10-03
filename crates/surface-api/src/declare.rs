//! Declarations
//!
//! Owned Lua edge values.

use mlua::{FromLua, Lua, LuaSerdeExt, Table, Value};
use mlua_extras::typed::Type;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::BTreeMap;

use confit_model::document::{Data, Document, StructuredFormat};
use confit_model::routes::Route;
use confit_surface_face::mismatch::received;
use confit_surface_face::translate::json_value;

use crate::error::ApiError;

/// One serializable value holding null with scalars and tables.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
pub struct SerializableValue(pub serde_json::Value);

impl mlua_extras::typed::Typed for SerializableValue {
    fn ty() -> Type {
        json_value()
    }
}

impl FromLua for SerializableValue {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}

/// Text body for plain text documents.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextBody {
    /// The exact file text.
    pub text: String,
}

confit_surface_face::typed!(TextBody);

impl FromLua for TextBody {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}

/// Structured body for rendered data documents.
#[derive(Debug, Clone, PartialEq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StructuredBody {
    /// The serialization format name.
    #[schemars(with = "String")]
    pub format: StructuredFormat,
    /// Holds the structured data table.
    pub data: BTreeMap<String, SerializableValue>,
}

confit_surface_face::typed!(StructuredBody);

impl FromLua for StructuredBody {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        lua.from_value(value)
    }
}

/// One document body in either shape.
#[derive(Debug, Clone, PartialEq, JsonSchema)]
#[serde(untagged)]
pub enum Body {
    /// Plain text payload.
    Text(TextBody),
    /// Rendered data payload.
    Structured(StructuredBody),
}

confit_surface_face::typed!(Body);

impl FromLua for Body {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let keys = match &value {
            Value::Table(table) => body_keys(table)?,
            _ => {
                return Err(ApiError::Mismatch {
                    slot: "body".to_owned(),
                    want: "a body table".to_owned(),
                    received: received(&value).to_owned(),
                }
                .into());
            }
        };
        if keys == ["text"] {
            return Ok(Self::Text(TextBody::from_lua(value, lua)?));
        }
        if keys == ["data", "format"] {
            return Ok(Self::Structured(StructuredBody::from_lua(value, lua)?));
        }
        Err(ApiError::Mismatch {
            slot: "body".to_owned(),
            want: "a 'text' body or a 'format' with 'data' body".to_owned(),
            received: keys.join(", "),
        }
        .into())
    }
}

/// One document declaration.
#[derive(Debug, Clone)]
pub struct Declaration {
    /// The late-bound destination route.
    pub destination: Route,
    /// The file payload in either shape.
    pub body: Body,
}

impl Declaration {
    /// Builds one declaration from a route with a body.
    pub fn new(destination: Route, body: Body) -> Self {
        Self { destination, body }
    }

    /// Reads the destination route.
    pub fn destination(&self) -> &Route {
        &self.destination
    }

    /// Reads the file payload in either shape.
    pub fn body(&self) -> &Body {
        &self.body
    }

    /// Freezes one declaration into its document.
    pub fn into_document(self) -> Document {
        let data = match self.body {
            Body::Text(found) => Data::Text {
                content: found.text,
                mode: None,
                unmanaged: false,
            },
            Body::Structured(found) => Data::Structured {
                format: found.format,
                data: found
                    .data
                    .into_iter()
                    .map(|(key, value)| (key, value.0))
                    .collect(),
            },
        };
        Document::new(self.destination, data)
    }
}

/// One profile in return order.
///
/// Missing returns read empty.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    /// Declarations in profile return order.
    pub documents: Vec<Declaration>,
}

impl Profile {
    /// Builds one profile from declarations in return order.
    pub fn new(documents: Vec<Declaration>) -> Self {
        Self { documents }
    }

    /// Reads the declarations in profile return order.
    pub fn documents(&self) -> &[Declaration] {
        &self.documents
    }
}

/// Reads the sorted string keys of one body table.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string keys.
fn body_keys(table: &Table) -> mlua::Result<Vec<String>> {
    let mut keys = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        match key {
            Value::String(text) => keys.push(text.to_string_lossy()),
            _ => {
                return Err(ApiError::Mismatch {
                    slot: "body key".to_owned(),
                    want: "a string key".to_owned(),
                    received: received(&key).to_owned(),
                }
                .into());
            }
        }
    }
    keys.sort();
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua_extras::typed::{Type, Typed};

    #[test]
    fn body_face_unions_member_faces() {
        assert_eq!(
            Body::ty(),
            Type::union(vec![
                Type::named("TextBody".to_owned()),
                Type::named("StructuredBody".to_owned()),
            ])
        );
    }
}
