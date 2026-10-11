//! Face
//! Declaration traits for Lua table and userdata faces.

use std::borrow::Cow;

use crate::registry::Registry;

/// One documented table field.
///
/// The descriptor carries the optional marker from
/// the shape declaration to stub emission.
pub struct FieldDescriptor {
    /// The field name.
    pub name: &'static str,
    /// The field documentation.
    pub documentation: &'static str,
    /// The field face expression.
    pub face: Cow<'static, str>,
    /// The optional marker.
    pub optional: bool,
}

/// One documented union variant.
///
/// The descriptor carries one member from the union
/// declaration to stub emission.
pub struct VariantDescriptor {
    /// The variant name.
    pub name: &'static str,
    /// The variant documentation.
    pub documentation: &'static str,
    /// The variant face expression.
    pub face: Cow<'static, str>,
}

/// Face kind marker.
///
/// Every declaration carries one marker from
/// derive to stub emission.
pub enum FaceKind {
    /// Table shape.
    Table,
    /// Userdata handle.
    Userdata,
}

/// Bound type face.
///
/// - Every implementor answers one stable Lua name.
/// - Every implementor answers one stub block.
/// - Every implementor declares one kind marker.
/// - Collection registers the face with every
///   referenced face exactly once.
pub trait Face {
    /// Return the Lua name.
    fn face_name() -> String;
    /// Return the stub block.
    fn face_stub() -> String;
    /// Return the kind marker.
    fn kind() -> FaceKind;
    /// Register the face with every referenced face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}

/// Bound namespace face.
///
/// - Every implementor answers one dotted Lua path.
/// - Every implementor answers one documentation string.
/// - Collection registers the namespace with every
///   member face exactly once.
pub trait Namespace {
    /// Return the dotted Lua path.
    fn namespace() -> &'static str;
    /// Return the documentation text.
    fn documentation() -> &'static str;
    /// Register the namespace with every member face.
    ///
    /// # Errors
    ///
    /// - [`crate::error::BindingError`] for repeated registration.
    fn collect(collector: &mut Registry) -> crate::error::Result<()>;
}

macro_rules! primitive_face {
    ($type:ty, $face:literal) => {
        impl Face for $type {
            fn face_name() -> String {
                $face.to_owned()
            }
            fn face_stub() -> String {
                String::new()
            }
            fn kind() -> FaceKind {
                FaceKind::Table
            }
            fn collect(_collector: &mut Registry) -> crate::error::Result<()> {
                Ok(())
            }
        }
    };
}

primitive_face!(String, "string");
primitive_face!(bool, "boolean");
primitive_face!(i64, "integer");
primitive_face!(u64, "integer");
primitive_face!(f64, "number");
primitive_face!(mlua::Function, "fun()");
primitive_face!(mlua::Value, "any");

impl Face for () {
    fn face_name() -> String {
        "nil".to_owned()
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        FaceKind::Table
    }
    fn collect(_collector: &mut Registry) -> crate::error::Result<()> {
        Ok(())
    }
}

impl<First: Face, Second: Face> Face for (First, Second) {
    fn face_name() -> String {
        format!("{}, {}", First::face_name(), Second::face_name())
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        First::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        First::collect(collector)?;
        Second::collect(collector)
    }
}

impl<T: Face> Face for Option<T> {
    fn face_name() -> String {
        format!("{}?", parenthesized(&T::face_name()))
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        T::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        T::collect(collector)
    }
}

impl<T: Face> Face for Vec<T> {
    fn face_name() -> String {
        format!("{}[]", parenthesized(&T::face_name()))
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        T::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        T::collect(collector)
    }
}

impl<T: Face> Face for Box<T> {
    fn face_name() -> String {
        T::face_name()
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        T::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        T::collect(collector)
    }
}

fn parenthesized(face: &str) -> String {
    if face.contains(" | ") {
        format!("({face})")
    } else {
        face.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding_macros::Face;
    use mlua::{Lua, Value};

    use crate::value::Decode;

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
    fn untagged_inline_synthesizes_documented_payload_class() {
        let stub = Condition_EnvEq::face_stub();
        assert_eq!(Condition_EnvEq::face_name(), "Condition_EnvEq");
        assert!(stub.contains("@class Condition_EnvEq"));
        assert!(stub.contains("Equality check."));
        assert!(stub.contains("@field key string The variable name."));
        assert!(stub.contains("@field value string The expected value."));
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
    fn untagged_union_face_references_payload_class() {
        let stub = Condition::face_stub();
        assert!(stub.contains("Condition_EnvEq"));
        let mut collector = Registry::new();
        Condition::collect(&mut collector).unwrap();
        assert!(collector.contains("Condition"));
        assert!(collector.contains("Condition_EnvEq"));
    }

    #[test]
    fn untagged_tuple_arm_keeps_inner_face() {
        let stub = Condition::face_stub();
        assert!(stub.contains("Condition[]"));
        assert!(!stub.contains("Condition_All"));
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
    fn tagged_inline_synthesizes_documented_payload_class() {
        let stub = TaggedCond_EnvEq::face_stub();
        assert_eq!(TaggedCond_EnvEq::face_name(), "TaggedCond_EnvEq");
        assert!(stub.contains("@class TaggedCond_EnvEq"));
        assert!(stub.contains("Equality check."));
        assert!(stub.contains("@field key string The variable name."));
        let union = TaggedCond::face_stub();
        assert!(union.contains("TaggedCond_EnvEq"));
        assert!(union.contains("env_eq"));
        assert!(!union.contains("TaggedCond_All"));
        let lua = Lua::new();
        let inner = lua.create_table().unwrap();
        inner.set("key", "K").unwrap();
        let outer = lua.create_table().unwrap();
        outer.set("env_eq", inner).unwrap();
        let decoded = TaggedCond::decode(&Value::Table(outer.clone()), &lua).unwrap();
        assert!(matches!(&decoded, TaggedCond::EnvEq { key } if key == "K"));
        let second = lua.create_table().unwrap();
        second.set(1, outer).unwrap();
        let outer_all = lua.create_table().unwrap();
        outer_all.set("all", second).unwrap();
        let nested = TaggedCond::decode(&Value::Table(outer_all), &lua).unwrap();
        assert!(matches!(&nested, TaggedCond::All(items) if items.len() == 1));
    }

    #[test]
    fn missing_docs_still_collect_payload_class() {
        let mut collector = Registry::new();
        NoDocs::collect(&mut collector).unwrap();
        assert!(collector.contains("NoDocs_Bare"));
        assert!(NoDocs::face_stub().contains("NoDocs_Bare"));
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
