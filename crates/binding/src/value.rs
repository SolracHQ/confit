//! Value
//! Lua value translation for bound faces.

use std::fmt::{Display, Formatter};

use mlua::{Lua, Table, Value};

/// One value mismatch with its field path.
///
/// The field path from the failing
/// value to the conversion error.
#[derive(Debug, Clone)]
pub struct Mismatch {
    /// The dotted field path.
    pub path: String,
    /// The wanted face expression.
    pub want: String,
    /// The found Lua type name.
    pub found: String,
}

impl Display for Mismatch {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}: want {}, found {}",
            self.path, self.want, self.found
        )
    }
}

/// Value translation for one bound face.
///
/// - Every implementor decodes Lua values of its documented
///   shape and rejects every other shape.
/// - Every failure carries the full field path.
pub trait Decode: Sized {
    /// Decode one Lua value into the bound face.
    ///
    /// - Tables decode field by field in declaration order.
    /// - First failure aborts the decode.
    ///
    /// # Errors
    ///
    /// - [`Mismatch`] for values outside the documented shape.
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch>;

    /// Decode one Lua value with sibling keys ignored.
    ///
    /// - Table faces accept sibling keys beside their own keys.
    /// - Other faces decode without reading the list.
    /// - Flatten holders pass their sibling keys through this list.
    ///
    /// # Errors
    ///
    /// - [`Mismatch`] for values outside the documented shape.
    fn decode_ignored(value: &Value, lua: &Lua, _ignored: &[&str]) -> Result<Self, Mismatch> {
        Self::decode(value, lua)
    }
}

/// Value encoding for one bound face.
///
/// - Every implementor renders values of its documented shape.
/// - Every implementor answers owned Lua values alone.
pub trait Encode {
    /// Encode the bound face into a Lua value.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for Lua allocation failures.
    fn encode(&self, lua: &Lua) -> mlua::Result<Value>;
}

/// Read the single entry of one single-key table.
///
/// # Errors
///
/// - [`Mismatch`] for tables outside the single-entry shape.
pub fn single_entry(name: &str, table: &Table) -> Result<(String, Value), Mismatch> {
    let mut entries = table.pairs::<String, Value>();
    let found = entries
        .next()
        .transpose()
        .map_err(|error| Mismatch {
            path: name.to_owned(),
            want: "a readable entry".to_owned(),
            found: error.to_string(),
        })?
        .ok_or_else(|| Mismatch {
            path: name.to_owned(),
            want: "one entry".to_owned(),
            found: "an empty table".to_owned(),
        })?;
    if entries.next().is_some() {
        return Err(Mismatch {
            path: name.to_owned(),
            want: "one entry".to_owned(),
            found: "a full table".to_owned(),
        });
    }
    Ok(found)
}

/// Read one Lua value as its table.
///
/// # Errors
///
/// - [`Mismatch`] for values outside the table shape.
pub fn table_value(value: &Value, shape: &str) -> Result<Table, Mismatch> {
    value.as_table().cloned().ok_or_else(|| Mismatch {
        path: shape.to_owned(),
        want: "a table".to_owned(),
        found: value.type_name().to_owned(),
    })
}

/// Reject unknown string keys on one table.
///
/// # Errors
///
/// - [`Mismatch`] for tables holding unknown fields.
pub fn check_keys(table: &Table, shape: &str, allowed: &[&str]) -> Result<(), Mismatch> {
    check_keys_ignored(table, shape, allowed, &[])
}

/// Reject unknown string keys outside the allowed and ignored lists.
///
/// - Keys in either list pass, every other key fails.
/// - Flatten holders pass their sibling keys as the ignored list.
/// - Overlapping lists fail as outer and inner key collisions.
///
/// # Errors
///
/// - [`Mismatch`] for tables holding unknown fields.
/// - [`Mismatch`] for keys present in both lists.
pub fn check_keys_ignored(
    table: &Table,
    shape: &str,
    allowed: &[&str],
    ignored: &[&str],
) -> Result<(), Mismatch> {
    for key in allowed {
        if ignored.contains(key) {
            return Err(Mismatch {
                path: shape.to_owned(),
                want: "distinct outer and inner keys".to_owned(),
                found: format!("collision on '{key}'"),
            });
        }
    }
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|error| Mismatch {
            path: shape.to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        match key {
            Value::String(text) => {
                let name = text.to_string_lossy();
                if !allowed.contains(&name.as_str()) && !ignored.contains(&name.as_str()) {
                    return Err(Mismatch {
                        path: shape.to_owned(),
                        want: format!("one of {}", allowed.join(", ")),
                        found: format!("unknown field '{name}'"),
                    });
                }
            }
            _ => {
                return Err(Mismatch {
                    path: shape.to_owned(),
                    want: "a string key".to_owned(),
                    found: key.type_name().to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// Collect the sorted string keys of one table.
///
/// # Errors
///
/// - [`Mismatch`] for tables with unreadable entries.
pub fn table_keys(table: &Table, shape: &str) -> Result<Vec<String>, Mismatch> {
    let mut keys = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|error| Mismatch {
            path: shape.to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        if let Value::String(text) = key {
            keys.push(text.to_string_lossy());
        }
    }
    keys.sort();
    Ok(keys)
}

/// One untagged routing outcome across variant key families.
///
/// The matched family index with stranger keys.
pub enum FamilyRoute {
    /// All input keys sit in exactly one family.
    Match(usize),
    /// Input keys land outside every known family.
    Stranger(Vec<String>),
    /// Several families share the input keys.
    Ambiguous,
}

/// Route one untagged key list across variant key families.
pub fn route_family(input: &[String], families: &[&[&str]]) -> FamilyRoute {
    let mut clean = Vec::new();
    for (index, family) in families.iter().enumerate() {
        let covers = input.iter().all(|key| family.contains(&key.as_str()));
        let touches = input.iter().any(|key| family.contains(&key.as_str()));
        if covers && touches {
            clean.push(index);
        }
    }
    let mut stranger = Vec::new();
    for key in input {
        let known = families.iter().any(|family| family.contains(&key.as_str()));
        if !known && !stranger.contains(key) {
            stranger.push(key.clone());
        }
    }
    if clean.len() == 1 {
        return FamilyRoute::Match(clean[0]);
    }
    if stranger.is_empty() {
        FamilyRoute::Ambiguous
    } else {
        FamilyRoute::Stranger(stranger)
    }
}

/// Read one table field with a missing-field error.
///
/// # Errors
///
/// - [`Mismatch`] for tables with unreadable fields.
pub fn read_field(table: &Table, shape: &str, field: &str) -> Result<Value, Mismatch> {
    table.get::<Value>(field).map_err(|error| Mismatch {
        path: format!("{shape}.{field}"),
        want: "a readable field".to_owned(),
        found: error.to_string(),
    })
}

/// Decode one field through its convert function.
///
/// - Convert functions access the value with Lua.
/// - Failures carry the field path.
///
/// # Errors
///
/// - [`Mismatch`] for values the convert function rejects.
pub fn decode_proxy<T>(
    lua: &Lua,
    value: &Value,
    path: &str,
    convert: fn(&Lua, &Value) -> Result<T, Mismatch>,
) -> Result<T, Mismatch> {
    convert(lua, value).map_err(|error| Mismatch {
        path: path.to_owned(),
        want: error.want,
        found: error.found,
    })
}

/// Read one table as its strict sequence values in key order.
///
/// # Errors
///
/// - [`Mismatch`] for tables outside the sequence shape.
pub fn decode_seq(table: &Table, name: &str) -> Result<Vec<Value>, Mismatch> {
    let mut items: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|error| Mismatch {
            path: name.to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        match key {
            Value::Integer(index) => items.push((index, item)),
            _ => {
                return Err(Mismatch {
                    path: name.to_owned(),
                    want: "integer sequence keys".to_owned(),
                    found: key.type_name().to_owned(),
                });
            }
        }
    }
    items.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in items.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(Mismatch {
                path: name.to_owned(),
                want: "contiguous keys from 1".to_owned(),
                found: format!("key {index}"),
            });
        }
    }
    Ok(items.into_iter().map(|(_, item)| item).collect())
}

/// Take one positional call argument with index advance.
pub fn take_arg(arguments: &[Value], next: &mut usize) -> Option<Value> {
    let found = arguments.get(*next).cloned().unwrap_or(Value::Nil);
    *next += 1;
    match found {
        Value::Nil => None,
        _ => Some(found),
    }
}

macro_rules! primitive_decode {
    ($type:ty, $face:literal, $want:literal, $body:expr) => {
        impl Decode for $type {
            fn decode(value: &Value, _lua: &Lua) -> Result<Self, Mismatch> {
                let body: fn(&Value) -> Option<Self> = $body;
                body(value).ok_or_else(|| Mismatch {
                    path: $face.to_owned(),
                    want: $want.to_owned(),
                    found: value.type_name().to_owned(),
                })
            }
        }
    };
}

primitive_decode!(String, "string", "a string", |value: &Value| match value {
    Value::String(text) => Some(text.to_string_lossy()),
    _ => None,
});
primitive_decode!(bool, "boolean", "a boolean", |value: &Value| value
    .as_boolean());
primitive_decode!(i64, "integer", "an integer", |value: &Value| value
    .as_integer());
primitive_decode!(
    u32,
    "integer",
    "a non-negative integer",
    |value: &Value| match value.as_integer() {
        Some(found) if found >= 0 && found <= u32::MAX as i64 => Some(found as u32),
        _ => None,
    }
);
primitive_decode!(
    u64,
    "integer",
    "a non-negative integer",
    |value: &Value| match value.as_integer() {
        Some(found) if found >= 0 => Some(found as u64),
        _ => None,
    }
);
primitive_decode!(f64, "number", "a number", |value: &Value| match value {
    Value::Integer(found) => Some(*found as f64),
    Value::Number(found) => Some(*found),
    _ => None,
});
primitive_decode!(
    mlua::Function,
    "function",
    "a function",
    |value: &Value| match value {
        Value::Function(found) => Some(found.clone()),
        _ => None,
    }
);

impl<T: Decode> Decode for Option<T> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
        match value {
            Value::Nil => Ok(None),
            _ => T::decode(value, lua).map(Some),
        }
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
        let table = table_value(value, "sequence")?;
        let items = decode_seq(&table, "sequence")?;
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                T::decode(item, lua).map_err(|error| Mismatch {
                    path: format!("entry {}: {}", index + 1, error.path),
                    want: error.want,
                    found: error.found,
                })
            })
            .collect()
    }
}

impl<T: Decode> Decode for Box<T> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
        T::decode(value, lua).map(Box::new)
    }
}

/// Decode one string keyed map with recursive values.
///
/// - String keys alone decode with other keys rejected.
/// - Values decode through their bound face in key order.
///
/// # Errors
///
/// - [`Mismatch`] for tables outside the map shape.
fn decode_map<V: Decode>(
    value: &Value,
    lua: &Lua,
    shape: &str,
) -> Result<Vec<(String, V)>, Mismatch> {
    let table = table_value(value, shape)?;
    let mut pairs: Vec<(String, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|error| Mismatch {
            path: shape.to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        match key {
            Value::String(text) => pairs.push((text.to_string_lossy(), item)),
            _ => {
                return Err(Mismatch {
                    path: shape.to_owned(),
                    want: "a string key".to_owned(),
                    found: key.type_name().to_owned(),
                });
            }
        }
    }
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut decoded: Vec<(String, V)> = Vec::new();
    for (key, item) in &pairs {
        let found = V::decode(item, lua).map_err(|error| Mismatch {
            path: format!("{shape}.{key}: {}", error.path),
            want: error.want,
            found: error.found,
        })?;
        decoded.push((key.clone(), found));
    }
    Ok(decoded)
}

impl<V: Decode> Decode for std::collections::HashMap<String, V> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
        decode_map::<V>(value, lua, "map").map(|pairs| pairs.into_iter().collect())
    }
}

impl<V: Decode> Decode for std::collections::BTreeMap<String, V> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
        decode_map::<V>(value, lua, "map").map(|pairs| pairs.into_iter().collect())
    }
}

impl Decode for Value {
    fn decode(value: &Value, _lua: &Lua) -> Result<Self, Mismatch> {
        Ok(value.clone())
    }
}

impl Decode for serde_json::Value {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, Mismatch> {
        use mlua::LuaSerdeExt as _;
        lua.from_value(value.clone()).map_err(|error| Mismatch {
            path: "value".to_owned(),
            want: "a JSON value".to_owned(),
            found: error.to_string(),
        })
    }
}

impl Decode for () {
    fn decode(value: &Value, _lua: &Lua) -> Result<Self, Mismatch> {
        match value {
            Value::Nil => Ok(()),
            _ => Err(Mismatch {
                path: "nil".to_owned(),
                want: "nil".to_owned(),
                found: value.type_name().to_owned(),
            }),
        }
    }
}

macro_rules! primitive_encode {
    ($type:ty, $body:expr) => {
        impl Encode for $type {
            fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
                let body: fn(&Self, &Lua) -> mlua::Result<Value> = $body;
                body(self, lua)
            }
        }
    };
}

primitive_encode!(bool, |value: &bool, _lua: &Lua| Ok(Value::Boolean(*value)));
primitive_encode!(i8, |value: &i8, _lua: &Lua| Ok(Value::Integer(i64::from(
    *value
))));
primitive_encode!(i16, |value: &i16, _lua: &Lua| Ok(Value::Integer(
    i64::from(*value)
)));
primitive_encode!(i32, |value: &i32, _lua: &Lua| Ok(Value::Integer(
    i64::from(*value)
)));
primitive_encode!(i64, |value: &i64, _lua: &Lua| Ok(Value::Integer(*value)));
primitive_encode!(u8, |value: &u8, _lua: &Lua| Ok(Value::Integer(i64::from(
    *value
))));
primitive_encode!(u16, |value: &u16, _lua: &Lua| Ok(Value::Integer(
    i64::from(*value)
)));
primitive_encode!(u32, |value: &u32, _lua: &Lua| Ok(Value::Integer(
    i64::from(*value)
)));
primitive_encode!(u64, |value: &u64, _lua: &Lua| Ok(Value::Integer(
    *value as i64
)));
primitive_encode!(f32, |value: &f32, _lua: &Lua| Ok(Value::Number(f64::from(
    *value
))));
primitive_encode!(f64, |value: &f64, _lua: &Lua| Ok(Value::Number(*value)));
primitive_encode!(String, |value: &String, lua: &Lua| lua
    .create_string(value)
    .map(Value::String));
primitive_encode!(&str, |value: &&str, lua: &Lua| lua
    .create_string(value)
    .map(Value::String));

impl Encode for () {
    fn encode(&self, _lua: &Lua) -> mlua::Result<Value> {
        Ok(Value::Nil)
    }
}

impl<T: Encode> Encode for Option<T> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        match self {
            Some(found) => found.encode(lua),
            None => Ok(Value::Nil),
        }
    }
}

impl<T: Encode> Encode for Vec<T> {
    fn encode(&self, lua: &Lua) -> mlua::Result<Value> {
        let sequence = lua.create_table()?;
        for (index, item) in self.iter().enumerate() {
            sequence.set(index + 1, item.encode(lua)?)?;
        }
        Ok(Value::Table(sequence))
    }
}

impl Encode for Value {
    fn encode(&self, _lua: &Lua) -> mlua::Result<Value> {
        Ok(self.clone())
    }
}

impl Encode for Table {
    fn encode(&self, _lua: &Lua) -> mlua::Result<Value> {
        Ok(Value::Table(self.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding_macros::Face;

    // Untagged routing pins live here. Macro shape tests pin docs alone.

    /// Union of inline and tuple arms.
    #[derive(Face)]
    #[face(table, untagged)]
    enum Condition {
        /// Equality check.
        EnvEq {
            /// The variable name.
            key: String,
            /// The expected value.
            value: String,
        },
        /// Conjunction.
        All(Vec<Condition>),
        /// Empty marker.
        Nothing,
    }

    /// Tagged union of inline and tuple arms.
    #[derive(Face)]
    #[face(table, tagged)]
    enum TaggedCond {
        /// Equality check.
        EnvEq {
            /// The variable name.
            key: String,
        },
        /// Conjunction.
        All(Vec<TaggedCond>),
    }

    /// Document union of one inline arm with one tuple arm.
    #[derive(Debug, Face)]
    #[face(table, untagged)]
    enum Document {
        /// Plain text payload.
        Text {
            /// The exact file text.
            content: String,
        },
        /// Structured payload.
        Shaped(ShapedPayload),
    }

    /// Structured payload for tuple-arm routing.
    #[derive(Debug, Face)]
    #[face(table)]
    struct ShapedPayload {
        /// Entries under shaping.
        entries: Vec<String>,
    }

    #[test]
    fn untagged_inline_table_decodes_variant_fields() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("key", "FOO").unwrap();
        table.set("value", "BAR").unwrap();
        let decoded = Condition::decode(&Value::Table(table), &lua).unwrap();
        assert!(matches!(
            &decoded,
            Condition::EnvEq { key, value } if key == "FOO" && value == "BAR"
        ));
    }

    #[test]
    fn untagged_tuple_arm_decodes_inner_payload() {
        let lua = Lua::new();
        let inner = lua.create_table().unwrap();
        inner.set("key", "A").unwrap();
        inner.set("value", "B").unwrap();
        let sequence = lua.create_table().unwrap();
        sequence.set(1, inner).unwrap();
        let decoded = Condition::decode(&Value::Table(sequence), &lua).unwrap();
        assert!(matches!(&decoded, Condition::All(items) if items.len() == 1));
    }

    #[test]
    fn tuple_union_decodes_both_arms() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hi").unwrap();
        let decoded = Document::decode(&Value::Table(table), &lua).unwrap();
        assert!(
            matches!(&decoded, Document::Text { content } if content.as_str() == "hi"),
            "inline arm decodes"
        );
        let entries = lua.create_table().unwrap();
        entries.set(1, "one").unwrap();
        let shaped = lua.create_table().unwrap();
        shaped.set("entries", entries).unwrap();
        let decoded = Document::decode(&Value::Table(shaped), &lua).unwrap();
        assert!(
            matches!(&decoded, Document::Shaped(ShapedPayload { entries }) if entries.len() == 1),
            "tuple arm decodes"
        );
    }

    #[test]
    fn ambiguous_table_keeps_union_error() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        assert!(
            Document::decode(&Value::Table(table), &lua).is_err(),
            "bare table fails"
        );
        assert!(
            Document::decode(&Value::Integer(1), &lua).is_err(),
            "foreign value fails"
        );
        let entries = lua.create_table().unwrap();
        entries.set(1, 1).unwrap();
        let shaped = lua.create_table().unwrap();
        shaped.set("entries", entries).unwrap();
        assert!(
            Document::decode(&Value::Table(shaped), &lua).is_err(),
            "foreign tuple fails"
        );
    }

    #[test]
    fn tagged_inline_table_decodes_single_key_payload() {
        let lua = Lua::new();
        let inner = lua.create_table().unwrap();
        inner.set("key", "K").unwrap();
        let outer = lua.create_table().unwrap();
        outer.set("env_eq", inner).unwrap();
        let decoded = TaggedCond::decode(&Value::Table(outer.clone()), &lua).unwrap();
        assert!(matches!(&decoded, TaggedCond::EnvEq { key } if key == "K"));
        let sequence = lua.create_table().unwrap();
        sequence.set(1, outer).unwrap();
        let outer_all = lua.create_table().unwrap();
        outer_all.set("all", sequence).unwrap();
        let nested = TaggedCond::decode(&Value::Table(outer_all), &lua).unwrap();
        assert!(matches!(&nested, TaggedCond::All(items) if items.len() == 1));
    }

    #[test]
    fn unit_encodes_nil() {
        let lua = Lua::new();
        assert!(matches!(().encode(&lua).unwrap(), Value::Nil));
    }

    /// Inner document body for flatten tests.
    #[derive(Debug, PartialEq, Face)]
    #[face(table)]
    struct FlatInner {
        /// The exact file text.
        content: String,
    }

    /// Holder pairing one outer field with a flattened body.
    #[derive(Debug, PartialEq, Face)]
    #[face(table)]
    struct FlatHolder {
        /// The document path.
        path: String,
        /// The flattened body.
        #[face(flatten)]
        body: FlatInner,
    }

    /// Holder with a sibling key colliding with the inner body.
    #[derive(Debug, Face)]
    #[face(table)]
    struct ClashingHolder {
        /// The clashing outer field.
        content: String,
        /// The flattened body.
        #[face(flatten)]
        body: FlatInner,
    }

    /// Untagged body with two inline arms for flatten routing tests.
    #[derive(Debug, PartialEq, Face)]
    #[face(table, untagged)]
    enum FlatData {
        /// Text arm.
        Text {
            /// The exact file text.
            content: String,
        },
        /// Count arm.
        Count {
            /// The entry count.
            count: u64,
        },
    }

    /// Holder flattening one untagged body.
    #[derive(Debug, PartialEq, Face)]
    #[face(table)]
    struct FlatUnionHolder {
        /// The document path.
        path: String,
        /// The flattened body.
        #[face(flatten)]
        data: FlatData,
    }

    /// Union with one skipped arm.
    #[derive(Debug, PartialEq, Face)]
    #[face(table, untagged)]
    enum MaybeSealed {
        /// Live payload.
        Live {
            /// The exact file text.
            content: String,
        },
        /// Sealed payload.
        #[face(skip_decode, skip_encode)]
        Sealed {
            /// The content digest.
            digest: String,
        },
    }

    #[test]
    fn flat_struct_round_trips_beside_sibling_keys() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("path", "doc/a").unwrap();
        table.set("content", "hello").unwrap();
        let decoded = FlatHolder::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded,
            FlatHolder {
                path: "doc/a".to_owned(),
                body: FlatInner {
                    content: "hello".to_owned(),
                },
            }
        );
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("holder encodes a table");
        };
        let path: String = back.get("path").unwrap();
        let content: String = back.get("content").unwrap();
        assert_eq!((path.as_str(), content.as_str()), ("doc/a", "hello"));
        let round = FlatHolder::decode(&Value::Table(back), &lua).unwrap();
        assert_eq!(round, decoded);
    }

    #[test]
    fn flat_struct_rejects_unknown_keys() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("path", "doc/a").unwrap();
        table.set("content", "hello").unwrap();
        table.set("bogus", 1).unwrap();
        assert!(
            FlatHolder::decode(&Value::Table(table), &lua).is_err(),
            "foreign key fails"
        );
    }

    #[test]
    fn colliding_sibling_keys_fail_the_decode() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hello").unwrap();
        assert!(
            ClashingHolder::decode(&Value::Table(table), &lua).is_err(),
            "clashing keys fail"
        );
    }

    #[test]
    fn ignored_keys_pass_without_collisions() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hello").unwrap();
        table.set("path", "doc/a").unwrap();
        check_keys_ignored(&table, "FlatInner", &["content"], &["path"]).unwrap();
        assert!(
            check_keys_ignored(&table, "FlatInner", &["content"], &["content"]).is_err(),
            "clashing ignored keys fail"
        );
    }

    #[test]
    fn untagged_arms_route_through_flatten() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("path", "doc/a").unwrap();
        table.set("content", "hello").unwrap();
        let decoded = FlatUnionHolder::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded,
            FlatUnionHolder {
                path: "doc/a".to_owned(),
                data: FlatData::Text {
                    content: "hello".to_owned(),
                },
            }
        );
        let count_table = lua.create_table().unwrap();
        count_table.set("path", "doc/b").unwrap();
        count_table.set("count", 3).unwrap();
        let decoded = FlatUnionHolder::decode(&Value::Table(count_table), &lua).unwrap();
        assert_eq!(
            decoded,
            FlatUnionHolder {
                path: "doc/b".to_owned(),
                data: FlatData::Count { count: 3 },
            }
        );
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("holder encodes a table");
        };
        let count: u64 = back.get("count").unwrap();
        assert_eq!(count, 3);
        let round = FlatUnionHolder::decode(&Value::Table(back), &lua).unwrap();
        assert_eq!(round, decoded);
    }

    #[test]
    fn skipped_arms_stay_out_of_decode_and_encode() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hello").unwrap();
        let decoded = MaybeSealed::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded,
            MaybeSealed::Live {
                content: "hello".to_owned(),
            }
        );
        let sealed_table = lua.create_table().unwrap();
        sealed_table.set("digest", "aa").unwrap();
        assert!(
            MaybeSealed::decode(&Value::Table(sealed_table), &lua).is_err(),
            "sealed arm stays out of decode"
        );
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("live arms encode a table");
        };
        let content: String = back.get("content").unwrap();
        assert_eq!(content, "hello");
        let sealed = MaybeSealed::Sealed {
            digest: "aa".to_owned(),
        };
        assert!(sealed.encode(&lua).is_err(), "sealed arms refuse encode");
    }
}
