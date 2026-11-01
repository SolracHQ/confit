//! Runtime shape data used by the `Shape` derive.
//!
//! The derive builds these values from Rust declarations. The generated code
//! passes them to the decoder, encoder, and stub generator.

use std::borrow::Cow;

use crate::generator::Generator;

/// Adds a shape and every shape it references to a generator.
pub type CollectFn = fn(&mut Generator);

/// A collector for shapes with no references.
pub fn no_collect(_collector: &mut Generator) {}

/// One field in a Lua table.
///
/// The derive builds a field from each Rust struct field. The field carries the
/// Lua key, the type expression, and the collector for any shape the type
/// references.
#[derive(Clone, Debug)]
pub struct Field {
    /// The Lua key.
    pub name: &'static str,
    /// The stub description.
    pub description: &'static str,
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// Whether the key may be absent.
    pub optional: bool,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for Field {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.shape == other.shape
            && self.optional == other.optional
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One parameter in a Lua function signature.
#[derive(Clone, Debug)]
pub struct Parameter {
    /// The Lua parameter label.
    pub name: &'static str,
    /// The stub description.
    pub description: &'static str,
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// Whether the argument may be absent.
    pub optional: bool,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for Parameter {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.shape == other.shape
            && self.optional == other.optional
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One return value in a Lua function signature.
#[derive(Clone, Debug)]
pub struct Return {
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// The stub description.
    pub description: &'static str,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for Return {
    fn eq(&self, other: &Self) -> bool {
        self.shape == other.shape
            && self.description == other.description
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One variant in a Lua alias.
///
/// One alternative in a Lua alias, with an optional payload table.
#[derive(Clone, Debug)]
pub struct Variant {
    /// The Lua variant name.
    pub name: &'static str,
    /// The stub description.
    pub description: &'static str,
    /// The Lua type expression.
    pub shape: Cow<'static, str>,
    /// The payload fields in order.
    pub fields: Vec<Field>,
    /// Adds every referenced shape to the generator.
    pub collect: CollectFn,
}

impl PartialEq for Variant {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.description == other.description
            && self.shape == other.shape
            && self.fields == other.fields
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// The Lua shape of a Rust type.
///
/// Each variant carries the data needed to decode, encode, and emit a kind of
/// Lua value.
#[derive(Clone, Debug, PartialEq)]
pub enum ShapeData {
    /// A Lua primitive such as a string, number, or boolean.
    Primitive {
        /// The stub description.
        description: &'static str,
    },
    /// A Lua table with named fields.
    Table {
        /// The stub description.
        description: &'static str,
        /// The table fields.
        fields: Vec<Field>,
    },
    /// A Lua alias over a fixed set of variants.
    Alias {
        /// The stub description.
        description: &'static str,
        /// The alias variants.
        variants: Vec<Variant>,
    },
    /// A Lua function with typed parameters and returns.
    Function {
        /// The stub description.
        description: &'static str,
        /// The function parameters.
        parameters: Vec<Parameter>,
        /// The function return values.
        returns: Vec<Return>,
    },
    /// A Lua userdata object with methods.
    Object {
        /// The stub description.
        description: &'static str,
    },
}

/// The Lua form of a Rust type.
///
/// The `Shape` derive is the supported way to build this implementation.
/// Decode and encode hooks cover values that need custom conversion.
pub trait Shape {
    /// Returns the Lua type name.
    fn name() -> String;
    /// Returns the shape data.
    fn shape_data() -> ShapeData;
    /// Adds the shape and every referenced shape to the generator.
    fn collect(collector: &mut Generator);
}

/// A Lua module with a dotted name and a set of member shapes.
///
/// `lua_module("...")` builds this surface on the type.
pub trait Module {
    /// Returns the dotted Lua module path.
    fn module() -> &'static str;
    /// Returns the stub description.
    fn description() -> &'static str;
    /// Adds the module and its member shapes to the generator.
    fn collect(collector: &mut Generator);
}

macro_rules! primitive_shape {
    ($type:ty, $shape:literal) => {
        impl Shape for $type {
            fn name() -> String {
                $shape.to_owned()
            }
            fn shape_data() -> ShapeData {
                ShapeData::Primitive { description: "" }
            }
            fn collect(_collector: &mut Generator) {}
        }
    };
}

primitive_shape!(String, "string");
primitive_shape!(std::path::PathBuf, "string");
primitive_shape!(bool, "boolean");
primitive_shape!(i64, "integer");
primitive_shape!(u32, "integer");
primitive_shape!(u64, "integer");
primitive_shape!(f64, "number");
primitive_shape!(mlua::Function, "fun()");
primitive_shape!(mlua::Table, "table");
primitive_shape!(mlua::Value, "any");
primitive_shape!(serde_json::Value, "any");

impl Shape for () {
    fn name() -> String {
        "nil".to_owned()
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(_collector: &mut Generator) {}
}

impl<First: Shape, Second: Shape> Shape for (First, Second) {
    fn name() -> String {
        format!("{}, {}", First::name(), Second::name())
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        First::collect(collector);
        Second::collect(collector);
    }
}

impl<T: Shape> Shape for Option<T> {
    fn name() -> String {
        format!("{}?", parenthesized(&T::name()))
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        T::collect(collector);
    }
}

impl<T: Shape> Shape for Vec<T> {
    fn name() -> String {
        format!("{}[]", parenthesized(&T::name()))
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        T::collect(collector);
    }
}

impl<T: Shape, const N: usize> Shape for [T; N] {
    fn name() -> String {
        format!("{}[]", parenthesized(&T::name()))
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        T::collect(collector);
    }
}

impl<T: Shape> Shape for Box<T> {
    fn name() -> String {
        T::name()
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        T::collect(collector);
    }
}

impl<K: Shape, V: Shape> Shape for std::collections::HashMap<K, V> {
    fn name() -> String {
        format!("{{[{}]: {}}}", K::name(), V::name())
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        K::collect(collector);
        V::collect(collector);
    }
}

impl<K: Shape, V: Shape> Shape for std::collections::BTreeMap<K, V> {
    fn name() -> String {
        format!("{{[{}]: {}}}", K::name(), V::name())
    }
    fn shape_data() -> ShapeData {
        ShapeData::Primitive { description: "" }
    }
    fn collect(collector: &mut Generator) {
        K::collect(collector);
        V::collect(collector);
    }
}

impl ShapeData {
    /// Returns the stub description.
    pub fn description(&self) -> &'static str {
        match self {
            Self::Primitive { description }
            | Self::Table { description, .. }
            | Self::Alias { description, .. }
            | Self::Function { description, .. }
            | Self::Object { description } => description,
        }
    }

    /// Adds every shape referenced by this shape to the generator.
    pub fn collect_referenced(&self, collector: &mut Generator) {
        match self {
            Self::Primitive { .. } | Self::Object { .. } => {}
            Self::Table { fields, .. } => {
                for field in fields {
                    (field.collect)(collector);
                }
            }
            Self::Alias { variants: arms, .. } => {
                for arm in arms {
                    (arm.collect)(collector);
                }
            }
            Self::Function {
                parameters,
                returns,
                ..
            } => {
                for parameter in parameters {
                    (parameter.collect)(collector);
                }
                for returned in returns {
                    (returned.collect)(collector);
                }
            }
        }
    }
}

fn parenthesized(shape: &str) -> String {
    if shape.contains(" | ") || shape.starts_with("fun(") {
        format!("({shape})")
    } else {
        shape.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Shape, lua_methods};
    use mlua::{Lua, Value};
    use std::collections::{BTreeMap, HashMap};

    use crate::error::DecodeError;
    use crate::value::{Decode, Encode};

    /// Table shape for map convert tests.
    #[derive(Debug, Shape)]
    #[shape(table)]
    struct MapTable {
        /// Entries keyed by name.
        #[shape(key_decode = upper_key, value_decode = exclaim_value, default)]
        entries: HashMap<String, String>,
    }

    /// Converts a map key to uppercase.
    fn upper_key(lua: &Lua, value: &Value) -> Result<String, DecodeError> {
        String::decode(value, lua).map(|key| key.to_uppercase())
    }

    /// Converts a map value to its exclaimed form.
    fn exclaim_value(lua: &Lua, value: &Value) -> Result<String, DecodeError> {
        String::decode(value, lua).map(|item| format!("{item}!"))
    }

    /// Table shape with a Rust-filled field beside its Lua shape.
    #[derive(Debug, Shape)]
    #[shape(table)]
    struct IgnoredTable {
        /// The kept Lua field.
        kept: String,
        /// The Rust-filled field missing from Lua.
        #[shape(ignore)]
        filled: std::time::Duration,
    }

    /// Alias with a Rust-filled inline field.
    #[derive(Shape)]
    #[shape(table, untagged)]
    enum IgnoredUnion {
        /// Text payload with a Rust-filled count.
        Text {
            /// The exact file text.
            content: String,
            /// The Rust-filled count.
            #[shape(ignore)]
            count: u64,
        },
        /// Plain sequence payload.
        Shaped(Vec<String>),
    }

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

    /// Tagged alias of inline and tuple variants.
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

    #[derive(Shape)]
    #[shape(table, untagged)]
    enum NoDocs {
        Bare { key: String },
        Many(Vec<NoDocs>),
    }

    #[test]
    fn untagged_inline_variant_synthesizes_described_payload_table() {
        assert_eq!(Condition_EnvEq::name(), "Condition_EnvEq");
        match Condition_EnvEq::shape_data() {
            ShapeData::Table {
                description,
                fields,
            } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "key");
                assert_eq!(fields[1].name, "value");
                assert_eq!(description, "Equality check.");
            }
            _ => panic!("expected table data for payload"),
        }
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
    fn untagged_alias_shape_references_payload_table() {
        match Condition::shape_data() {
            ShapeData::Alias { variants: arms, .. } => {
                assert_eq!(arms.len(), 3);
                assert_eq!(arms[0].shape.as_ref(), "Condition_EnvEq");
            }
            _ => panic!("expected alias data for alias"),
        }
        let mut collector = Generator::new();
        Condition::collect(&mut collector);
        assert!(collector.contains("Condition"));
        assert!(collector.contains("Condition_EnvEq"));
        assert!(collector.generate().contains("Condition_EnvEq"));
    }

    #[test]
    fn untagged_tuple_variant_keeps_inner_shape() {
        match Condition::shape_data() {
            ShapeData::Alias { variants: arms, .. } => {
                assert_eq!(arms[1].shape.as_ref(), "Condition[]");
                assert_eq!(arms[2].shape.as_ref(), "nil");
            }
            _ => panic!("expected alias data for alias"),
        }
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
    fn typed_alias_inline_variant_synthesizes_described_payload_table() {
        assert_eq!(TaggedCond_EnvEq::name(), "TaggedCond_EnvEq");
        match TaggedCond_EnvEq::shape_data() {
            ShapeData::Table { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "key");
            }
            _ => panic!("expected table data for payload"),
        }
        match TaggedCond_EnvEq::shape_data() {
            ShapeData::Table { description, .. } => {
                assert_eq!(description, "Equality check.");
            }
            _ => panic!("expected table data for payload"),
        }
        match TaggedCond::shape_data() {
            ShapeData::Alias { variants: arms, .. } => assert_eq!(arms.len(), 2),
            _ => panic!("expected alias data for typed_alias"),
        }
        let mut collector = Generator::new();
        TaggedCond::collect(&mut collector);
        let text = collector.generate();
        assert!(text.contains("TaggedCond_EnvEq"));
        assert!(text.contains("env_eq"));
        assert!(!text.contains("TaggedCond_All"));
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
    fn map_shapes_read_member_shapes() {
        assert_eq!(
            HashMap::<String, String>::name(),
            "{[string]: string}".to_owned()
        );
        assert_eq!(
            BTreeMap::<String, NoDocs>::name(),
            "{[string]: NoDocs}".to_owned()
        );
        let mut collector = Generator::new();
        MapTable::collect(&mut collector);
        assert!(collector.contains("MapTable"));
        assert!(collector.generate().contains("{[string]: string}"));
    }

    #[test]
    fn map_converts_decode_entries() {
        let lua = Lua::new();
        let entries = lua.create_table().unwrap();
        entries.set("alpha", "one").unwrap();
        let table = lua.create_table().unwrap();
        table.set("entries", entries).unwrap();
        let decoded = MapTable::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(
            decoded.entries.get("ALPHA").map(String::as_str),
            Some("one!")
        );
        let bare = lua.create_table().unwrap();
        let decoded = MapTable::decode(&Value::Table(bare), &lua).unwrap();
        assert!(decoded.entries.is_empty());
    }

    #[test]
    fn map_convert_failures_keep_dotted_paths() {
        let lua = Lua::new();
        let entries = lua.create_table().unwrap();
        entries.set("alpha", 1).unwrap();
        let table = lua.create_table().unwrap();
        table.set("entries", entries).unwrap();
        assert!(
            MapTable::decode(&Value::Table(table), &lua).is_err(),
            "foreign entry fails"
        );
    }

    #[test]
    fn ignored_fields_skip_data_with_defaults_through_codecs() {
        match IgnoredTable::shape_data() {
            ShapeData::Table { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "kept");
            }
            _ => panic!("expected table data for table"),
        }
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("kept", "hi").unwrap();
        let decoded = IgnoredTable::decode(&Value::Table(table), &lua).unwrap();
        assert_eq!(decoded.kept, "hi");
        assert_eq!(decoded.filled, std::time::Duration::default());
        let encoded = decoded.encode(&lua).unwrap();
        let Value::Table(back) = encoded else {
            panic!("table encodes a table");
        };
        let kept: String = back.get("kept").unwrap();
        assert_eq!(kept, "hi");
        assert!(
            !back.contains_key("filled").unwrap(),
            "the encoding skips the Rust field"
        );
        let round = IgnoredTable::decode(&Value::Table(back), &lua).unwrap();
        assert_eq!(round.filled, std::time::Duration::default());
    }

    #[test]
    fn ignored_fields_reject_wrong_keys() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("kept", "hi").unwrap();
        table.set("filled", 1).unwrap();
        assert!(
            IgnoredTable::decode(&Value::Table(table), &lua).is_err(),
            "foreign key fails"
        );
    }

    /// String alias shape for data tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(table, str_alias)]
    enum Shade {
        /// Dark tone.
        Dark,
        /// Light tone.
        Light,
    }

    /// Live table for callable data tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(table)]
    struct Process {
        /// The title.
        title: String,
    }

    /// Signature shape for data tests.
    #[allow(dead_code)]
    #[derive(Shape)]
    #[shape(function)]
    struct Args {
        /// Target name.
        name: String,
        /// Live process under running.
        #[shape(borrowed)]
        process: Process,
    }

    /// Userdata object for data tests.
    #[derive(Clone, Shape)]
    #[shape(object)]
    struct Handle {
        seed: String,
    }

    #[lua_methods]
    impl Handle {
        /// Returns the seed.
        ///
        /// # Returns
        ///
        /// - The stored seed.
        pub fn seed_name(&self) -> String {
            self.seed.clone()
        }
    }

    #[test]
    fn ignored_inline_fields_skip_data_with_defaults() {
        match IgnoredUnion_Text::shape_data() {
            ShapeData::Table { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "content");
            }
            _ => panic!("expected table data for payload"),
        }
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("content", "hi").unwrap();
        let decoded = IgnoredUnion::decode(&Value::Table(table), &lua).unwrap();
        match decoded {
            IgnoredUnion::Text { content, count } => {
                assert_eq!(content, "hi");
                assert_eq!(count, 0);
            }
            IgnoredUnion::Shaped(_) => panic!("content routes to the text variant"),
        }
    }

    /// Inner body for flatten data tests.
    #[derive(Debug, Shape)]
    #[shape(table)]
    struct FlatInner {
        /// The exact file text.
        content: String,
    }

    /// Table with a flattened body for data tests.
    #[derive(Debug, Shape)]
    #[shape(table)]
    struct FlatTable {
        /// The table path.
        path: String,
        /// The flattened body.
        #[shape(flatten)]
        body: FlatInner,
    }

    #[test]
    fn struct_data_keeps_fields_with_descriptions() {
        match IgnoredTable::shape_data() {
            ShapeData::Table {
                description,
                fields,
            } => {
                assert_eq!(
                    description,
                    "Table shape with a Rust-filled field beside its Lua shape."
                );
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "kept");
                assert_eq!(fields[0].shape.as_ref(), "string");
            }
            _ => panic!("expected table data for struct"),
        }
    }

    #[test]
    fn untagged_data_folds_variant_fields_in_order() {
        match Condition::shape_data() {
            ShapeData::Alias {
                description,
                variants: arms,
            } => {
                assert_eq!(description, "Alias of inline and tuple variants.");
                assert_eq!(arms.len(), 3);
                assert_eq!(arms[0].name, "EnvEq");
                assert_eq!(arms[0].fields.len(), 2);
                assert_eq!(arms[0].fields[0].name, "key");
                assert!(arms[1].fields.is_empty());
                assert!(arms[2].fields.is_empty());
                assert_eq!(arms[0].shape.as_ref(), "Condition_EnvEq");
            }
            _ => panic!("untagged expected alias data for alias"),
        }
    }

    #[test]
    fn typed_alias_data_keeps_variants_with_descriptions() {
        match TaggedCond::shape_data() {
            ShapeData::Alias {
                description,
                variants: arms,
            } => {
                assert_eq!(description, "Tagged alias of inline and tuple variants.");
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].name, "EnvEq");
            }
            _ => panic!("typed_alias expected alias data for alias"),
        }
    }

    #[test]
    fn str_alias_data_shows_quoted_shapes() {
        match Shade::shape_data() {
            ShapeData::Alias {
                description,
                variants: arms,
            } => {
                assert_eq!(description, "String alias shape for data tests.");
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].shape.as_ref(), "\"dark\"");
                assert_eq!(arms[1].shape.as_ref(), "\"light\"");
            }
            _ => panic!("expected alias data for str_alias"),
        }
    }

    #[test]
    fn signature_data_lists_params_without_returns() {
        match Args::shape_data() {
            ShapeData::Function {
                description,
                parameters,
                returns,
            } => {
                assert_eq!(description, "Signature shape for data tests.");
                assert_eq!(parameters.len(), 2);
                assert_eq!(parameters[0].name, "name");
                assert_eq!(parameters[1].name, "process");
                assert_eq!(parameters[1].shape.as_ref(), "Process");
                assert!(returns.is_empty());
            }
            _ => panic!("expected function data for signature shape"),
        }
    }

    #[test]
    fn object_data_keeps_descriptions() {
        match Handle::shape_data() {
            ShapeData::Object { description } => {
                assert_eq!(description, "Userdata object for data tests.");
            }
            _ => panic!("expected object data for object"),
        }
    }

    #[test]
    fn primitive_data_marks_primitives() {
        assert!(matches!(String::shape_data(), ShapeData::Primitive { .. }));
        assert!(matches!(bool::shape_data(), ShapeData::Primitive { .. }));
        assert!(matches!(
            Vec::<String>::shape_data(),
            ShapeData::Primitive { .. }
        ));
        assert!(matches!(
            Option::<String>::shape_data(),
            ShapeData::Primitive { .. }
        ));
    }

    #[test]
    fn flattened_table_data_keeps_outer_fields() {
        match FlatTable::shape_data() {
            ShapeData::Table { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "path");
                assert_eq!(fields[0].shape.as_ref(), "string");
            }
            _ => panic!("flat expected table data for table"),
        }
    }

    #[test]
    fn empty_cases_hold_no_fields_or_variants() {
        assert!(matches!(String::shape_data(), ShapeData::Primitive { .. }));
        assert!(matches!(Args::shape_data(), ShapeData::Function { .. }));
        assert!(matches!(Handle::shape_data(), ShapeData::Object { .. }));
        match Args::shape_data() {
            ShapeData::Function { returns, .. } => assert!(returns.is_empty()),
            _ => panic!("expected function data for callable"),
        }
        match Process::shape_data() {
            ShapeData::Table { fields, .. } => assert_eq!(fields.len(), 1),
            _ => panic!("expected table data for live process"),
        }
    }
}
