//! Value
//! Lua value translation for bound faces.

use std::fmt::{Display, Formatter};

use mlua::{Lua, Table, Value};

/// One value mismatch with its field path.
///
/// The mismatch carries the field path from the failing
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
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair.map_err(|error| Mismatch {
            path: shape.to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        match key {
            Value::String(text) => {
                let name = text.to_string_lossy();
                if !allowed.contains(&name.as_str()) {
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
/// - The convert function reads the value with Lua access.
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
    let mut slots: Vec<(i64, Value)> = Vec::new();
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair.map_err(|error| Mismatch {
            path: name.to_owned(),
            want: "readable entries".to_owned(),
            found: error.to_string(),
        })?;
        match key {
            Value::Integer(index) => slots.push((index, item)),
            _ => {
                return Err(Mismatch {
                    path: name.to_owned(),
                    want: "integer sequence keys".to_owned(),
                    found: key.type_name().to_owned(),
                });
            }
        }
    }
    slots.sort_by_key(|(index, _)| *index);
    for (position, (index, _)) in slots.iter().enumerate() {
        if *index != position as i64 + 1 {
            return Err(Mismatch {
                path: name.to_owned(),
                want: "contiguous keys from 1".to_owned(),
                found: format!("key {index}"),
            });
        }
    }
    Ok(slots.into_iter().map(|(_, item)| item).collect())
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

impl Decode for Value {
    fn decode(value: &Value, _lua: &Lua) -> Result<Self, Mismatch> {
        Ok(value.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding_macros::Face;

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

    #[derive(Face)]
    #[face(table, untagged)]
    enum NoDocs {
        Bare { key: String },
        Many(Vec<NoDocs>),
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
    fn missing_docs_inline_table_decodes() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("key", "K").unwrap();
        let decoded = NoDocs::decode(&Value::Table(table.clone()), &lua).unwrap();
        assert!(matches!(&decoded, NoDocs::Bare { key } if key == "K"));
        let sequence = lua.create_table().unwrap();
        sequence.set(1, table).unwrap();
        let nested = NoDocs::decode(&Value::Table(sequence), &lua).unwrap();
        assert!(matches!(&nested, NoDocs::Many(items) if items.len() == 1));
    }
}
