//! Face
//! Bound face traits for Lua tables and userdata.

use std::borrow::Cow;

use crate::registry::Registry;

/// One documented table field.
///
/// The optional marker from
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
/// One member from the union
/// declaration to stub emission.
pub struct VariantDescriptor {
    /// The variant name.
    pub name: &'static str,
    /// The variant documentation.
    pub documentation: &'static str,
    /// The variant face expression.
    pub face: Cow<'static, str>,
}

/// One documented function.
///
/// The function face from
/// the static declaration to stub emission.
pub struct FunctionDescriptor {
    /// The function name.
    pub name: &'static str,
    /// The function documentation.
    pub documentation: &'static str,
}

/// One documented function parameter.
///
/// The optional marker from
/// the static declaration to stub emission.
pub struct FunctionParameter {
    /// The parameter name.
    pub name: &'static str,
    /// The parameter documentation.
    pub documentation: &'static str,
    /// The parameter face expression.
    pub face: Cow<'static, str>,
    /// The optional marker.
    pub optional: bool,
}

/// One documented function return.
///
/// One return slot from the
/// static declaration to stub emission.
pub struct FunctionReturn {
    /// The return face expression.
    pub face: Cow<'static, str>,
    /// The return documentation.
    pub documentation: &'static str,
}

/// Face kind marker.
///
/// Every declaration carries one marker from
/// derive to stub emission.
pub enum FaceKind {
    /// Table shape.
    Table,
    /// Function shape.
    Function,
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
primitive_face!(u32, "integer");
primitive_face!(u64, "integer");
primitive_face!(f64, "number");
primitive_face!(mlua::Function, "fun()");
primitive_face!(mlua::Table, "table");
primitive_face!(mlua::Value, "any");
primitive_face!(serde_json::Value, "any");

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

impl<K: Face, V: Face> Face for std::collections::HashMap<K, V> {
    fn face_name() -> String {
        format!("{{[{}]: {}}}", K::face_name(), V::face_name())
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        V::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        K::collect(collector)?;
        V::collect(collector)
    }
}

impl<K: Face, V: Face> Face for std::collections::BTreeMap<K, V> {
    fn face_name() -> String {
        format!("{{[{}]: {}}}", K::face_name(), V::face_name())
    }
    fn face_stub() -> String {
        String::new()
    }
    fn kind() -> FaceKind {
        V::kind()
    }
    fn collect(collector: &mut Registry) -> crate::error::Result<()> {
        K::collect(collector)?;
        V::collect(collector)
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
    use std::collections::{BTreeMap, HashMap};

    use crate::value::{Decode, Encode, Mismatch};

    /// Holder face for map convert tests.
    #[derive(Debug, Face)]
    #[face(table)]
    struct MapHolder {
        /// Entries keyed by name.
        #[face(key_convert = upper_key, value_convert = exclaim_value, default)]
        entries: HashMap<String, String>,
    }

    /// Read one map key into its uppercase shape.
    fn upper_key(lua: &Lua, value: &Value) -> Result<String, Mismatch> {
        String::decode(value, lua).map(|key| key.to_uppercase())
    }

    /// Read one map value into its exclaimed shape.
    fn exclaim_value(lua: &Lua, value: &Value) -> Result<String, Mismatch> {
        String::decode(value, lua).map(|item| format!("{item}!"))
    }

    /// Holder face with one Rust-filled field beside its Lua shape.
    #[derive(Debug, Face)]
    #[face(table)]
    struct IgnoredHolder {
        /// The kept Lua field.
        kept: String,
        /// The Rust-filled field missing from Lua.
        #[face(ignore)]
        filled: std::time::Duration,
    }

    /// Union with one Rust-filled inline field.
    #[derive(Face)]
    #[face(table, untagged)]
    enum IgnoredUnion {
        /// Text payload with a Rust-filled count.
        Text {
            /// The exact file text.
            content: String,
            /// The Rust-filled count.
            #[face(ignore)]
            count: u64,
        },
        /// Plain sequence payload.
        Shaped(Vec<String>),
    }

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

    #[test]
    fn map_faces_read_member_faces() {
        assert_eq!(
            HashMap::<String, String>::face_name(),
            "{[string]: string}".to_owned()
        );
        assert_eq!(
            BTreeMap::<String, NoDocs>::face_name(),
            "{[string]: NoDocs}".to_owned()
        );
        let mut collector = Registry::new();
        MapHolder::collect(&mut collector).unwrap();
        assert!(collector.contains("MapHolder"));
        assert!(MapHolder::face_stub().contains("{[string]: string}"));
    }

    #[test]
    fn map_converts_decode_entries() {
        let lua = Lua::new();
        let entries = lua.create_table().unwrap();
        entries.set("alpha", "one").unwrap();
        let table = lua.create_table().unwrap();
        table.set("entries", entries).unwrap();
        let decoded = MapHolder::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded.entries.get("ALPHA").map(String::as_str),
            Some("one!")
        );
        let bare = lua.create_table().unwrap();
        let decoded = MapHolder::decode(&Value::Table(bare), &lua).unwrap();
        assert!(decoded.entries.is_empty());
    }

    #[test]
    fn map_convert_failures_keep_dotted_paths() {
        let lua = Lua::new();
        let entries = lua.create_table().unwrap();
        entries.set("alpha", 1).unwrap();
        let table = lua.create_table().unwrap();
        table.set("entries", entries).unwrap();
        let failure = MapHolder::decode(&Value::Table(table), &lua).unwrap_err();
        let text = failure.to_string();
        assert!(
            text.contains("MapHolder.entries"),
            "the text shows the field: {text}"
        );
        assert!(text.contains("a string"), "the text shows the want: {text}");
    }

    #[test]
    fn ignored_fields_skip_stubs_with_defaults_through_codecs() {
        let stub = IgnoredHolder::face_stub();
        assert!(stub.contains("kept"), "the stub shows the Lua field");
        assert!(
            !stub.contains("@field filled"),
            "the stub skips the Rust field"
        );
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("kept", "hi").unwrap();
        let decoded = IgnoredHolder::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(decoded.kept, "hi");
        assert_eq!(decoded.filled, std::time::Duration::default());
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("holder encodes a table");
        };
        let kept: String = back.get("kept").unwrap();
        assert_eq!(kept, "hi");
        assert!(
            !back.contains_key("filled").unwrap(),
            "the encoding skips the Rust field"
        );
        let round = IgnoredHolder::decode(&Value::Table(back), &lua).unwrap();
        assert_eq!(round.filled, std::time::Duration::default());
    }

    #[test]
    fn ignored_fields_reject_foreign_keys() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("kept", "hi").unwrap();
        table.set("filled", 1).unwrap();
        let failure = IgnoredHolder::decode(&Value::Table(table), &lua).unwrap_err();
        let text = failure.to_string();
        assert!(text.contains("filled"), "the text shows the key: {text}");
    }

    #[test]
    fn ignored_inline_fields_skip_stubs_with_defaults() {
        let stub = IgnoredUnion_Text::face_stub();
        assert!(stub.contains("content"), "the stub shows the Lua field");
        assert!(
            !stub.contains("@field count"),
            "the stub skips the Rust field"
        );
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hi").unwrap();
        let decoded = IgnoredUnion::decode(&Value::Table(table), &lua).unwrap();
        match decoded {
            IgnoredUnion::Text { content, count } => {
                assert_eq!(content, "hi");
                assert_eq!(count, 0);
            }
            IgnoredUnion::Shaped(_) => panic!("content routes to the text arm"),
        }
    }
}
