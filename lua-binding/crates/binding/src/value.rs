//! Decode and encode between Rust values and Lua values.
//!
//! The `Shape` derive implements both directions from the Rust declaration.
//! The helpers here serve the generated code for tables, sequences, maps, and
//! untagged variants.

use mlua::{Lua, Table, Value};

use super::error::{DecodeError, DecodeTag, Segment};

/// Converts a Lua value into a Rust value of a bound shape.
///
/// The `Shape` derive implements this for a bound type. A decode hook covers
/// custom conversion.
pub trait Decode: Sized {
    /// Decodes one Lua value.
    ///
    /// # Errors
    ///
    /// - [`DecodeError`] for a value outside the shape.
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError>;

    /// Decodes one Lua value while treating the named sibling keys as absent.
    ///
    /// # Errors
    ///
    /// - [`DecodeError`] for a value outside the shape.
    fn decode_ignored(value: &Value, lua: &Lua, _ignored: &[&str]) -> Result<Self, DecodeError> {
        Self::decode(value, lua)
    }
}

/// Converts a Rust value into a Lua value of a bound shape.
///
/// The `Shape` derive implements this for a bound type. An encode hook covers
/// custom conversion.
pub trait Encode {
    /// Encodes the value.
    ///
    /// # Errors
    ///
    /// - [`mlua::Error`] for a Lua allocation failure.
    fn encode(&self, lua: &Lua) -> mlua::Result<Value>;
}

/// Returns the key and value of a table that holds exactly one entry.
///
/// # Errors
///
/// - [`DecodeError`] for a table with zero or several entries.
/// - [`DecodeError`] for an unreadable entry.
pub fn single_entry(table: &Table) -> Result<(String, Value), DecodeError> {
    let mut entries = table.pairs::<String, Value>();
    let found = entries
        .next()
        .transpose()
        .map_err(|error| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "a readable entry".to_owned(),
                found: error.to_string(),
            },
        })?
        .ok_or_else(|| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "one entry".to_owned(),
                found: "an empty table".to_owned(),
            },
        })?;
    if entries.next().is_some() {
        return Err(DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "one entry".to_owned(),
                found: "a full table".to_owned(),
            },
        });
    }
    Ok(found)
}

/// Returns the table behind a Lua value.
///
/// # Errors
///
/// - [`DecodeError`] for a non-table value.
pub fn table_value(value: &Value) -> Result<Table, DecodeError> {
    value.as_table().cloned().ok_or_else(|| DecodeError {
        segments: Vec::new(),
        reason: DecodeTag::ShapeMismatch {
            require: "a table".to_owned(),
            found: value.type_name().to_owned(),
        },
    })
}

/// Rejects string keys outside the allowed set.
///
/// # Errors
///
/// - [`DecodeError`] for an unknown string key.
/// - [`DecodeError`] for a non-string key.
pub fn check_keys(table: &Table, allowed: &[&str]) -> Result<(), DecodeError> {
    check_keys_ignored(table, allowed, &[])
}

/// Rejects string keys outside the allowed and ignored sets.
///
/// A key in both sets is a collision.
///
/// # Errors
///
/// - [`DecodeError`] for an unknown string key.
/// - [`DecodeError`] for a non-string key.
/// - [`DecodeError`] for a key that is both allowed and ignored.
pub fn check_keys_ignored(
    table: &Table,
    allowed: &[&str],
    ignored: &[&str],
) -> Result<(), DecodeError> {
    for key in allowed {
        if ignored.contains(key) {
            return Err(DecodeError {
                segments: Vec::new(),
                reason: DecodeTag::ShapeMismatch {
                    require: "distinct outer and inner keys".to_owned(),
                    found: format!("collision on '{key}'"),
                },
            });
        }
    }
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|error| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "readable entries".to_owned(),
                found: error.to_string(),
            },
        })?;
        match key {
            Value::String(text) => {
                let name = text.to_string_lossy();
                if !allowed.contains(&name.as_str()) && !ignored.contains(&name.as_str()) {
                    return Err(DecodeError {
                        segments: Vec::new(),
                        reason: DecodeTag::ShapeMismatch {
                            require: format!("one of {}", allowed.join(", ")),
                            found: format!("unknown field '{name}'"),
                        },
                    });
                }
            }
            _ => {
                return Err(DecodeError {
                    segments: Vec::new(),
                    reason: DecodeTag::ShapeMismatch {
                        require: "a string key".to_owned(),
                        found: key.type_name().to_owned(),
                    },
                });
            }
        }
    }
    Ok(())
}

/// Returns the sorted string keys of a table.
///
/// # Errors
///
/// - [`DecodeError`] for an unreadable entry.
pub fn table_keys(table: &Table) -> Result<Vec<String>, DecodeError> {
    let mut keys = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|error| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "readable entries".to_owned(),
                found: error.to_string(),
            },
        })?;
        if let Value::String(text) = key {
            keys.push(text.to_string_lossy());
        }
    }
    keys.sort();
    Ok(keys)
}

/// The result of routing a key set to an untagged variant.
pub enum VariantRoute {
    /// One variant group covers every key.
    Match(usize),
    /// At least one key belongs to no group. The value lists those keys.
    Unknown(Vec<String>),
    /// No single group covers every key, and every key is known.
    Ambiguous,
}

/// Routes a key set to its untagged variant group.
///
/// A key set covered by exactly one group returns its index. Keys outside every group
/// return [`VariantRoute::Unknown`]. A key set that no single group covers
/// returns [`VariantRoute::Ambiguous`].
pub fn route_variant(input: &[String], groups: &[&[&str]]) -> VariantRoute {
    let mut clean = Vec::new();
    for (index, group) in groups.iter().enumerate() {
        let covers = input.iter().all(|key| group.contains(&key.as_str()));
        let touches = input.iter().any(|key| group.contains(&key.as_str()));
        if covers && touches {
            clean.push(index);
        }
    }
    let mut unknown = Vec::new();
    for key in input {
        let known = groups.iter().any(|group| group.contains(&key.as_str()));
        if !known && !unknown.contains(key) {
            unknown.push(key.clone());
        }
    }
    if clean.len() == 1 {
        return VariantRoute::Match(clean[0]);
    }
    if unknown.is_empty() {
        VariantRoute::Ambiguous
    } else {
        VariantRoute::Unknown(unknown)
    }
}

/// Returns the value of a table field.
///
/// A missing key returns `nil`.
///
/// # Errors
///
/// - [`DecodeError`] for an unreadable field.
pub fn read_field(table: &Table, field: &str) -> Result<Value, DecodeError> {
    table.get::<Value>(field).map_err(|error| DecodeError {
        segments: vec![Segment::Field(field.to_owned())],
        reason: DecodeTag::ShapeMismatch {
            require: "a readable field".to_owned(),
            found: error.to_string(),
        },
    })
}

/// Decodes a field through a convert function and prefixes any error with
/// the field name.
///
/// # Errors
///
/// - [`DecodeError`] for a value the convert function rejects.
pub fn decode_proxy<T>(
    lua: &Lua,
    value: &Value,
    field: &str,
    convert: fn(&Lua, &Value) -> Result<T, DecodeError>,
) -> Result<T, DecodeError> {
    convert(lua, value).map_err(|mut error| {
        error.segments.insert(0, Segment::Field(field.to_owned()));
        error
    })
}

/// Decodes a table as a sequence with contiguous 1-based integer keys.
///
/// # Errors
///
/// - [`DecodeError`] for a non-integer key.
/// - [`DecodeError`] for a gap in the key range.
/// - [`DecodeError`] for an unreadable entry.
pub fn decode_seq(table: &Table) -> Result<Vec<Value>, DecodeError> {
    let mut items: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|error| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "readable entries".to_owned(),
                found: error.to_string(),
            },
        })?;
        match key {
            Value::Integer(index) => items.push((index, item)),
            _ => {
                return Err(DecodeError {
                    segments: Vec::new(),
                    reason: DecodeTag::ShapeMismatch {
                        require: "integer sequence keys".to_owned(),
                        found: key.type_name().to_owned(),
                    },
                });
            }
        }
    }
    items.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in items.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(DecodeError {
                segments: Vec::new(),
                reason: DecodeTag::ShapeMismatch {
                    require: "contiguous keys from 1".to_owned(),
                    found: format!("key {index}"),
                },
            });
        }
    }
    Ok(items.into_iter().map(|(_, item)| item).collect())
}

/// Takes the next positional argument and advances the cursor.
///
/// A missing or `nil` argument returns `None`.
pub fn take_arg(arguments: &[Value], next: &mut usize) -> Option<Value> {
    let found = arguments.get(*next).cloned().unwrap_or(Value::Nil);
    *next += 1;
    match found {
        Value::Nil => None,
        _ => Some(found),
    }
}

macro_rules! primitive_decode {
    ($type:ty, $require:literal, $body:expr) => {
        impl Decode for $type {
            fn decode(value: &Value, _lua: &Lua) -> Result<Self, DecodeError> {
                let body: fn(&Value) -> Option<Self> = $body;
                body(value).ok_or_else(|| DecodeError {
                    segments: Vec::new(),
                    reason: DecodeTag::ShapeMismatch {
                        require: $require.to_owned(),
                        found: value.type_name().to_owned(),
                    },
                })
            }
        }
    };
}

primitive_decode!(String, "a string", |value: &Value| match value {
    Value::String(text) => Some(text.to_string_lossy()),
    _ => None,
});

impl Decode for std::path::PathBuf {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        String::decode(value, lua).map(std::path::PathBuf::from)
    }
}
primitive_decode!(bool, "a boolean", |value: &Value| value.as_boolean());
primitive_decode!(i64, "an integer", |value: &Value| value.as_integer());
primitive_decode!(
    u32,
    "a non-negative integer",
    |value: &Value| match value.as_integer() {
        Some(found) if found >= 0 && found <= u32::MAX as i64 => Some(found as u32),
        _ => None,
    }
);
primitive_decode!(
    u64,
    "a non-negative integer",
    |value: &Value| match value.as_integer() {
        Some(found) if found >= 0 => Some(found as u64),
        _ => None,
    }
);
primitive_decode!(f64, "a number", |value: &Value| match value {
    Value::Integer(found) => Some(*found as f64),
    Value::Number(found) => Some(*found),
    _ => None,
});
primitive_decode!(mlua::Function, "a function", |value: &Value| match value {
    Value::Function(found) => Some(found.clone()),
    _ => None,
});

impl<T: Decode> Decode for Option<T> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        match value {
            Value::Nil => Ok(None),
            _ => T::decode(value, lua).map(Some),
        }
    }
}

impl<T: Decode> Decode for Vec<T> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        let table = table_value(value)?;
        let items = decode_seq(&table)?;
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                T::decode(item, lua).map_err(|mut error| {
                    error.segments.insert(0, Segment::Index(index as u32 + 1));
                    error
                })
            })
            .collect()
    }
}

impl<T: Decode, const N: usize> Decode for [T; N] {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        let table = table_value(value)?;
        let items = decode_seq(&table)?;
        if items.len() != N {
            return Err(DecodeError {
                segments: Vec::new(),
                reason: DecodeTag::ShapeMismatch {
                    require: format!("an array of {N} values"),
                    found: format!("{} values", items.len()),
                },
            });
        }
        let decoded: Vec<T> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                T::decode(item, lua).map_err(|mut error| {
                    error.segments.insert(0, Segment::Index(index as u32 + 1));
                    error
                })
            })
            .collect::<Result<_, _>>()?;
        decoded.try_into().map_err(|found: Vec<T>| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: format!("an array of {N} values"),
                found: format!("{} values", found.len()),
            },
        })
    }
}

impl<T: Decode> Decode for Box<T> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        T::decode(value, lua).map(Box::new)
    }
}

/// Decodes a map with string keys.
///
/// # Errors
///
/// - [`DecodeError`] for tables outside the map shape.
fn decode_map<V: Decode>(value: &Value, lua: &Lua) -> Result<Vec<(String, V)>, DecodeError> {
    let table = table_value(value)?;
    let mut pairs: Vec<(String, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|error| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "readable entries".to_owned(),
                found: error.to_string(),
            },
        })?;
        match key {
            Value::String(text) => pairs.push((text.to_string_lossy(), item)),
            _ => {
                return Err(DecodeError {
                    segments: Vec::new(),
                    reason: DecodeTag::ShapeMismatch {
                        require: "a string key".to_owned(),
                        found: key.type_name().to_owned(),
                    },
                });
            }
        }
    }
    pairs.sort_by(|left, right| left.0.cmp(&right.0));
    let mut decoded: Vec<(String, V)> = Vec::new();
    for (key, item) in &pairs {
        let found = V::decode(item, lua).map_err(|mut error| {
            error.segments.insert(0, Segment::Field(key.clone()));
            error
        })?;
        decoded.push((key.clone(), found));
    }
    Ok(decoded)
}

impl<V: Decode> Decode for std::collections::HashMap<String, V> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        decode_map::<V>(value, lua).map(|pairs| pairs.into_iter().collect())
    }
}

impl<V: Decode> Decode for std::collections::BTreeMap<String, V> {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        decode_map::<V>(value, lua).map(|pairs| pairs.into_iter().collect())
    }
}

impl Decode for Value {
    fn decode(value: &Value, _lua: &Lua) -> Result<Self, DecodeError> {
        Ok(value.clone())
    }
}

impl Decode for serde_json::Value {
    fn decode(value: &Value, lua: &Lua) -> Result<Self, DecodeError> {
        use mlua::LuaSerdeExt as _;
        lua.from_value(value.clone()).map_err(|error| DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "a JSON value".to_owned(),
                found: error.to_string(),
            },
        })
    }
}

impl Decode for () {
    fn decode(value: &Value, _lua: &Lua) -> Result<Self, DecodeError> {
        match value {
            Value::Nil => Ok(()),
            _ => Err(DecodeError {
                segments: Vec::new(),
                reason: DecodeTag::ShapeMismatch {
                    require: "nil".to_owned(),
                    found: value.type_name().to_owned(),
                },
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

impl<T: Encode, const N: usize> Encode for [T; N] {
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
    use crate::Shape;
    use crate::trace::{LuaTrace, SourceSite};

    // Untagged routing pins live here. Macro shape tests cover the docs.

    /// Alias of inline and tuple variants.
    #[derive(Shape)]
    #[shape(table, untagged)]
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

    /// Typed alias of inline and tuple variants.
    #[derive(Shape)]
    #[shape(table, typed_alias)]
    enum TaggedCond {
        /// Equality check.
        EnvEq {
            /// The variable name.
            key: String,
        },
        /// Conjunction.
        All(Vec<TaggedCond>),
    }

    /// Process alias of an inline variant with a tuple variant.
    #[derive(Debug, Shape)]
    #[shape(table, untagged)]
    enum Process {
        /// Plain text payload.
        Text {
            /// The exact file text.
            content: String,
        },
        /// Structured payload.
        Shaped(ShapedPayload),
    }

    /// Structured payload for tuple-arm routing.
    #[derive(Debug, Shape)]
    #[shape(table)]
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
    fn untagged_tuple_variant_decodes_inner_payload() {
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
    fn tuple_alias_decodes_both_variants() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hi").unwrap();
        let decoded = Process::decode(&Value::Table(table), &lua).unwrap();
        assert!(
            matches!(&decoded, Process::Text { content } if content.as_str() == "hi"),
            "inline arm decodes"
        );
        let entries = lua.create_table().unwrap();
        entries.set(1, "one").unwrap();
        let shaped = lua.create_table().unwrap();
        shaped.set("entries", entries).unwrap();
        let decoded = Process::decode(&Value::Table(shaped), &lua).unwrap();
        assert!(
            matches!(&decoded, Process::Shaped(ShapedPayload { entries }) if entries.len() == 1),
            "tuple arm decodes"
        );
    }

    #[test]
    fn ambiguous_table_keeps_alias_error() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        assert!(
            Process::decode(&Value::Table(table), &lua).is_err(),
            "bare table fails"
        );
        assert!(
            Process::decode(&Value::Integer(1), &lua).is_err(),
            "foreign value fails"
        );
        let entries = lua.create_table().unwrap();
        entries.set(1, 1).unwrap();
        let shaped = lua.create_table().unwrap();
        shaped.set("entries", entries).unwrap();
        assert!(
            Process::decode(&Value::Table(shaped), &lua).is_err(),
            "foreign tuple fails"
        );
    }

    #[test]
    fn typed_alias_inline_table_decodes_single_key_payload() {
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

    /// Inner table body for flatten tests.
    #[derive(Debug, PartialEq, Shape)]
    #[shape(table)]
    struct FlatInner {
        /// The exact file text.
        content: String,
    }

    /// Table pairing an outer field with a flattened body.
    #[derive(Debug, PartialEq, Shape)]
    #[shape(table)]
    struct FlatTable {
        /// The table path.
        path: String,
        /// The flattened body.
        #[shape(flatten)]
        body: FlatInner,
    }

    /// Table with a sibling key colliding with the inner body.
    #[derive(Debug, Shape)]
    #[shape(table)]
    struct ClashingTable {
        /// The clashing outer field.
        content: String,
        /// The flattened body.
        #[shape(flatten)]
        body: FlatInner,
    }

    /// Untagged body with two inline variants for flatten routing tests.
    #[derive(Debug, PartialEq, Shape)]
    #[shape(table, untagged)]
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

    /// Table flattening an untagged body.
    #[derive(Debug, PartialEq, Shape)]
    #[shape(table)]
    struct FlatUnionTable {
        /// The table path.
        path: String,
        /// The flattened body.
        #[shape(flatten)]
        data: FlatData,
    }

    #[test]
    fn flat_struct_round_trips_beside_sibling_keys() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("path", "doc/a").unwrap();
        table.set("content", "hello").unwrap();
        let decoded = FlatTable::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded,
            FlatTable {
                path: "doc/a".to_owned(),
                body: FlatInner {
                    content: "hello".to_owned(),
                },
            }
        );
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("table encodes a table");
        };
        let path: String = back.get("path").unwrap();
        let content: String = back.get("content").unwrap();
        assert_eq!((path.as_str(), content.as_str()), ("doc/a", "hello"));
        let round = FlatTable::decode(&Value::Table(back), &lua).unwrap();
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
            FlatTable::decode(&Value::Table(table), &lua).is_err(),
            "foreign key fails"
        );
    }

    #[test]
    fn colliding_sibling_keys_fail_the_decode() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hello").unwrap();
        assert!(
            ClashingTable::decode(&Value::Table(table), &lua).is_err(),
            "clashing keys fail"
        );
    }

    #[test]
    fn ignored_keys_pass_without_collisions() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hello").unwrap();
        table.set("path", "doc/a").unwrap();
        check_keys_ignored(&table, &["content"], &["path"]).unwrap();
        assert!(
            check_keys_ignored(&table, &["content"], &["content"]).is_err(),
            "clashing ignored keys fail"
        );
    }

    #[test]
    fn untagged_variants_route_through_flatten() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("path", "doc/a").unwrap();
        table.set("content", "hello").unwrap();
        let decoded = FlatUnionTable::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded,
            FlatUnionTable {
                path: "doc/a".to_owned(),
                data: FlatData::Text {
                    content: "hello".to_owned(),
                },
            }
        );
        let count_table = lua.create_table().unwrap();
        count_table.set("path", "doc/b").unwrap();
        count_table.set("count", 3).unwrap();
        let decoded = FlatUnionTable::decode(&Value::Table(count_table), &lua).unwrap();
        assert_eq!(
            decoded,
            FlatUnionTable {
                path: "doc/b".to_owned(),
                data: FlatData::Count { count: 3 },
            }
        );
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("table encodes a table");
        };
        let count: u64 = back.get("count").unwrap();
        assert_eq!(count, 3);
        let round = FlatUnionTable::decode(&Value::Table(back), &lua).unwrap();
        assert_eq!(round, decoded);
    }

    #[test]
    fn vec_decode_marks_failing_position_from_one() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set(1, "ok").unwrap();
        table.set(2, 1).unwrap();
        match <Vec<String>>::decode(&Value::Table(table), &lua) {
            Ok(_) => panic!("foreign item passes"),
            Err(error) => {
                assert_eq!(error.segments, vec![Segment::Index(2)]);
                assert!(
                    matches!(error.reason, DecodeTag::ShapeMismatch { .. }),
                    "the reason keeps the shape: {}",
                    error.reason
                );
            }
        }
    }

    #[test]
    fn fixed_array_round_trips_nested_values() {
        let lua = Lua::new();
        let source = [[1_i64, 2], [3, 4]];
        let encoded = source.encode(&lua).unwrap();
        let decoded = <[[i64; 2]; 2]>::decode(&encoded, &lua).unwrap();
        assert_eq!(decoded, source);
    }

    #[test]
    fn fixed_array_rejects_wrong_length() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set(1, 1_i64).unwrap();
        match <[i64; 2]>::decode(&Value::Table(table), &lua) {
            Ok(_) => panic!("short array passes"),
            Err(error) => {
                assert_eq!(error.path(), "");
                assert!(error.to_string().contains("an array of 2 values"));
                assert!(error.to_string().contains("1 values"));
            }
        }
    }

    #[test]
    fn map_decode_marks_failing_field() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("good", "ok").unwrap();
        table.set("bad", 1).unwrap();
        match <std::collections::BTreeMap<String, String>>::decode(&Value::Table(table), &lua) {
            Ok(_) => panic!("foreign value passes"),
            Err(error) => {
                assert_eq!(error.segments, vec![Segment::Field("bad".to_owned())]);
                assert!(
                    matches!(error.reason, DecodeTag::ShapeMismatch { .. }),
                    "the reason keeps the shape: {}",
                    error.reason
                );
            }
        }
    }

    #[test]
    fn trace_capture_outside_lua_returns_none() {
        let lua = Lua::new();
        assert!(
            LuaTrace::capture(&lua).is_none(),
            "bare state holds no frames"
        );
        assert!(LuaTrace::default().0.is_empty(), "default holds no sites");
    }

    #[test]
    fn trace_capture_inside_lua_holds_file_and_line() {
        let lua = Lua::new();
        let probe = lua
            .create_function(|lua, ()| {
                let trace = LuaTrace::capture(lua).unwrap_or_default();
                let site = trace.0.first().cloned().unwrap_or(SourceSite {
                    file: String::new(),
                    line: String::new(),
                });
                Ok((site.file, site.line))
            })
            .unwrap();
        lua.globals().set("probe", probe).unwrap();
        let (file, line): (String, String) = lua
            .load("return probe()")
            .set_name("@probe_case.lua")
            .call(())
            .unwrap();
        assert!(
            file.contains("probe_case.lua"),
            "frame shows the chunk file"
        );
        assert_eq!(line, "1", "frame shows the chunk line");
    }

    #[test]
    fn trace_capture_inside_wrapper_holds_both_frames() {
        let lua = Lua::new();
        let probe = lua
            .create_function(|lua, ()| {
                let trace = LuaTrace::capture(lua).unwrap_or_default();
                Ok(trace
                    .0
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<String>>())
            })
            .unwrap();
        lua.globals().set("probe", probe).unwrap();
        let source = "local function wrap()\n  local out = probe()\n  return out\nend\nlocal done = wrap()\nreturn done";
        let frames: Vec<String> = lua
            .load(source)
            .set_name("@wrap_case.lua")
            .call(())
            .unwrap();
        assert_eq!(frames.len(), 2, "wrapper holds two frames");
        assert!(
            frames[0].contains("wrap_case.lua:2"),
            "inner frame shows the wrapper line"
        );
        assert!(
            frames[1].contains("wrap_case.lua:5"),
            "outer frame shows the chunk line"
        );
    }

    #[test]
    fn proxy_decode_marks_field_name() {
        let lua = Lua::new();
        match decode_proxy(&lua, &Value::Integer(1), "base", |lua, value| {
            String::decode(value, lua)
        }) {
            Ok(_) => panic!("foreign field passes"),
            Err(error) => {
                assert_eq!(error.segments, vec![Segment::Field("base".to_owned())]);
                match &error.reason {
                    DecodeTag::ShapeMismatch { require, found } => {
                        assert_eq!(require, "a string");
                        assert_eq!(found, "integer");
                    }
                    _ => panic!("proxy keeps the shape: {}", error.reason),
                }
            }
        }
    }
}
