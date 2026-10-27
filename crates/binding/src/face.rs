//! Bound face traits for Lua tables and userdata.

use std::borrow::Cow;

use crate::registry::Registry;

/// Collect one face with every referenced face.
pub type CollectFn = fn(&mut Registry);

/// Collect none.
pub fn no_collect(_collector: &mut Registry) {}

/// One documented table field.
#[derive(Clone, Debug)]
pub struct FieldDescriptor {
    /// The field name.
    pub name: &'static str,
    /// The field documentation.
    pub documentation: &'static str,
    /// The field face expression.
    pub face: Cow<'static, str>,
    /// The optional marker.
    pub optional: bool,
    /// The face collector.
    pub collect: CollectFn,
}

impl PartialEq for FieldDescriptor {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.documentation == other.documentation
            && self.face == other.face
            && self.optional == other.optional
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One documented function parameter.
#[derive(Clone, Debug)]
pub struct FunctionParameter {
    /// The parameter name.
    pub name: &'static str,
    /// The parameter documentation.
    pub documentation: &'static str,
    /// The parameter face expression.
    pub face: Cow<'static, str>,
    /// The optional marker.
    pub optional: bool,
    /// The face collector.
    pub collect: CollectFn,
}

impl PartialEq for FunctionParameter {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.documentation == other.documentation
            && self.face == other.face
            && self.optional == other.optional
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One documented function return.
#[derive(Clone, Debug)]
pub struct FunctionReturn {
    /// The return face expression.
    pub face: Cow<'static, str>,
    /// The return documentation.
    pub documentation: &'static str,
    /// The face collector.
    pub collect: CollectFn,
}

impl PartialEq for FunctionReturn {
    fn eq(&self, other: &Self) -> bool {
        self.face == other.face
            && self.documentation == other.documentation
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// One union arm.
#[derive(Clone, Debug)]
pub struct UnionArm {
    /// The variant name.
    pub name: &'static str,
    /// The variant documentation.
    pub documentation: &'static str,
    /// The variant face expression.
    pub face: Cow<'static, str>,
    /// The payload fields in order.
    pub fields: Vec<FieldDescriptor>,
    /// The face collector.
    pub collect: CollectFn,
    /// The alias skip marker.
    pub skipped: bool,
}

impl PartialEq for UnionArm {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.documentation == other.documentation
            && self.face == other.face
            && self.fields == other.fields
            && self.skipped == other.skipped
            && std::ptr::fn_addr_eq(self.collect, other.collect)
    }
}

/// Bound face snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum FaceData {
    /// Scalar stub shape.
    Scalar {
        /// The holder documentation.
        documentation: &'static str,
    },
    /// Table shape.
    Class {
        /// The holder documentation.
        documentation: &'static str,
        /// The table fields.
        fields: Vec<FieldDescriptor>,
    },
    /// Union shape.
    Union {
        /// The holder documentation.
        documentation: &'static str,
        /// The union arms with fields.
        arms: Vec<UnionArm>,
    },
    /// Function shape.
    Function {
        /// The holder documentation.
        documentation: &'static str,
        /// The function params.
        parameters: Vec<FunctionParameter>,
        /// The function returns.
        returns: Vec<FunctionReturn>,
    },
    /// Userdata handle shape.
    Userdata {
        /// The holder documentation.
        documentation: &'static str,
    },
}

/// Bound type face.
pub trait Face {
    /// Return the Lua name.
    fn face_name() -> String;
    /// Return the stub snapshot.
    fn face() -> FaceData;
    /// Register the face with every referenced face.
    fn collect(collector: &mut Registry);
}

/// Bound namespace face.
pub trait Namespace {
    /// Return the dotted Lua path.
    fn namespace() -> &'static str;
    /// Return the documentation text.
    fn documentation() -> &'static str;
    /// Register the namespace with every member face.
    fn collect(collector: &mut Registry);
}

macro_rules! primitive_face {
    ($type:ty, $face:literal) => {
        impl Face for $type {
            fn face_name() -> String {
                $face.to_owned()
            }
            fn face() -> FaceData {
                FaceData::Scalar { documentation: "" }
            }
            fn collect(_collector: &mut Registry) {}
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
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(_collector: &mut Registry) {}
}

impl<First: Face, Second: Face> Face for (First, Second) {
    fn face_name() -> String {
        format!("{}, {}", First::face_name(), Second::face_name())
    }
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(collector: &mut Registry) {
        First::collect(collector);
        Second::collect(collector);
    }
}

impl<T: Face> Face for Option<T> {
    fn face_name() -> String {
        format!("{}?", parenthesized(&T::face_name()))
    }
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(collector: &mut Registry) {
        T::collect(collector);
    }
}

impl<T: Face> Face for Vec<T> {
    fn face_name() -> String {
        format!("{}[]", parenthesized(&T::face_name()))
    }
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(collector: &mut Registry) {
        T::collect(collector);
    }
}

impl<T: Face> Face for Box<T> {
    fn face_name() -> String {
        T::face_name()
    }
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(collector: &mut Registry) {
        T::collect(collector);
    }
}

impl<K: Face, V: Face> Face for std::collections::HashMap<K, V> {
    fn face_name() -> String {
        format!("{{[{}]: {}}}", K::face_name(), V::face_name())
    }
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(collector: &mut Registry) {
        K::collect(collector);
        V::collect(collector);
    }
}

impl<K: Face, V: Face> Face for std::collections::BTreeMap<K, V> {
    fn face_name() -> String {
        format!("{{[{}]: {}}}", K::face_name(), V::face_name())
    }
    fn face() -> FaceData {
        FaceData::Scalar { documentation: "" }
    }
    fn collect(collector: &mut Registry) {
        K::collect(collector);
        V::collect(collector);
    }
}

impl FaceData {
    /// Return the holder documentation.
    pub fn documentation(&self) -> &'static str {
        match self {
            Self::Scalar { documentation }
            | Self::Class { documentation, .. }
            | Self::Union { documentation, .. }
            | Self::Function { documentation, .. }
            | Self::Userdata { documentation } => documentation,
        }
    }

    /// Collect every referenced face.
    pub fn collect_referenced(&self, collector: &mut Registry) {
        match self {
            Self::Scalar { .. } | Self::Userdata { .. } => {}
            Self::Class { fields, .. } => {
                for field in fields {
                    (field.collect)(collector);
                }
            }
            Self::Union { arms, .. } => {
                for arm in arms {
                    if arm.skipped {
                        continue;
                    }
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

fn parenthesized(face: &str) -> String {
    if face.contains(" | ") || face.starts_with("fun(") {
        format!("({face})")
    } else {
        face.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding_macros::{Face, face_impl};
    use mlua::{Lua, Value};
    use std::collections::{BTreeMap, HashMap};

    use crate::fault::FaceFault;
    use crate::value::{Decode, Encode};

    /// Holder face for map convert tests.
    #[derive(Debug, Face)]
    #[face(table)]
    struct MapHolder {
        /// Entries keyed by name.
        #[face(key_convert = upper_key, value_convert = exclaim_value, default)]
        entries: HashMap<String, String>,
    }

    /// Read one map key into its uppercase shape.
    fn upper_key(lua: &Lua, value: &Value) -> Result<String, FaceFault> {
        String::decode(value, lua).map(|key| key.to_uppercase())
    }

    /// Read one map value into its exclaimed shape.
    fn exclaim_value(lua: &Lua, value: &Value) -> Result<String, FaceFault> {
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
        assert_eq!(Condition_EnvEq::face_name(), "Condition_EnvEq");
        match Condition_EnvEq::face() {
            FaceData::Class {
                documentation,
                fields,
            } => {
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "key");
                assert_eq!(fields[1].name, "value");
                assert_eq!(documentation, "Equality check.");
            }
            _ => panic!("expected class data for payload"),
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
    fn untagged_union_face_references_payload_class() {
        match Condition::face() {
            FaceData::Union { arms, .. } => {
                assert_eq!(arms.len(), 3);
                assert_eq!(arms[0].face.as_ref(), "Condition_EnvEq");
            }
            _ => panic!("expected union data for union"),
        }
        let mut collector = Registry::new();
        Condition::collect(&mut collector);
        assert!(collector.contains("Condition"));
        assert!(collector.contains("Condition_EnvEq"));
        assert!(collector.stub_text().contains("Condition_EnvEq"));
    }

    #[test]
    fn untagged_tuple_arm_keeps_inner_face() {
        match Condition::face() {
            FaceData::Union { arms, .. } => {
                assert_eq!(arms[1].face.as_ref(), "Condition[]");
                assert_eq!(arms[2].face.as_ref(), "nil");
            }
            _ => panic!("expected union data for union"),
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
    fn tagged_inline_synthesizes_documented_payload_class() {
        assert_eq!(TaggedCond_EnvEq::face_name(), "TaggedCond_EnvEq");
        match TaggedCond_EnvEq::face() {
            FaceData::Class { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "key");
            }
            _ => panic!("expected class data for payload"),
        }
        match TaggedCond_EnvEq::face() {
            FaceData::Class { documentation, .. } => {
                assert_eq!(documentation, "Equality check.");
            }
            _ => panic!("expected class data for payload"),
        }
        match TaggedCond::face() {
            FaceData::Union { arms, .. } => assert_eq!(arms.len(), 2),
            _ => panic!("expected union data for tagged"),
        }
        let mut collector = Registry::new();
        TaggedCond::collect(&mut collector);
        let text = collector.stub_text();
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
        MapHolder::collect(&mut collector);
        assert!(collector.contains("MapHolder"));
        assert!(collector.stub_text().contains("{[string]: string}"));
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
        assert!(
            MapHolder::decode(&Value::Table(table), &lua).is_err(),
            "foreign entry fails"
        );
    }

    #[test]
    fn ignored_fields_skip_data_with_defaults_through_codecs() {
        match IgnoredHolder::face() {
            FaceData::Class { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "kept");
            }
            _ => panic!("expected class data for holder"),
        }
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
        assert!(
            IgnoredHolder::decode(&Value::Table(table), &lua).is_err(),
            "foreign key fails"
        );
    }

    /// String face for data tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(table, string)]
    enum Shade {
        /// Dark tone.
        Dark,
        /// Light tone.
        Light,
    }

    /// Live table for callback data tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(table)]
    struct LiveDoc {
        /// The title.
        title: String,
    }

    /// Callback shape for data tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(callback_shape)]
    struct CallArgs {
        /// Target route.
        route: String,
        /// Live document under patching.
        #[face(live)]
        document: LiveDoc,
    }

    /// Userdata handle for data tests.
    #[derive(Clone, Face)]
    #[face(userdata)]
    struct Handle {
        seed: String,
    }

    #[face_impl]
    impl Handle {
        /// Return the seed.
        ///
        /// # Returns
        ///
        /// - The stored seed.
        pub fn seed_name(&self) -> String {
            self.seed.clone()
        }
    }

    /// Union with a skipped arm for data tests.
    #[allow(dead_code)]
    #[derive(Face)]
    #[face(table, untagged)]
    enum SkippedUnion {
        /// Live payload.
        Live {
            /// The content.
            content: String,
        },
        /// Sealed payload.
        #[face(skip_decode, skip_encode)]
        Sealed(String),
    }

    #[test]
    fn ignored_inline_fields_skip_data_with_defaults() {
        match IgnoredUnion_Text::face() {
            FaceData::Class { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "content");
            }
            _ => panic!("expected class data for payload"),
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
            IgnoredUnion::Shaped(_) => panic!("content routes to the text arm"),
        }
    }

    /// Inner body for flatten data tests.
    #[derive(Debug, Face)]
    #[face(table)]
    struct FlatInner {
        /// The exact file text.
        content: String,
    }

    /// Holder with one flattened body for data tests.
    #[derive(Debug, Face)]
    #[face(table)]
    struct FlatHolder {
        /// The document path.
        path: String,
        /// The flattened body.
        #[face(flatten)]
        body: FlatInner,
    }

    #[test]
    fn struct_data_kept_fields_with_docs() {
        match IgnoredHolder::face() {
            FaceData::Class {
                documentation,
                fields,
            } => {
                assert_eq!(
                    documentation,
                    "Holder face with one Rust-filled field beside its Lua shape."
                );
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "kept");
                assert_eq!(fields[0].face.as_ref(), "string");
            }
            _ => panic!("expected class data for struct"),
        }
    }

    #[test]
    fn untagged_data_folds_arm_fields_in_order() {
        match Condition::face() {
            FaceData::Union {
                documentation,
                arms,
            } => {
                assert_eq!(documentation, "Union of inline and tuple arms.");
                assert_eq!(arms.len(), 3);
                assert_eq!(arms[0].name, "EnvEq");
                assert_eq!(arms[0].fields.len(), 2);
                assert_eq!(arms[0].fields[0].name, "key");
                assert!(arms[1].fields.is_empty());
                assert!(arms[2].fields.is_empty());
                assert_eq!(arms[0].face.as_ref(), "Condition_EnvEq");
            }
            _ => panic!("untagged expected union data for union"),
        }
    }

    #[test]
    fn tagged_data_kept_arms_with_docs() {
        match TaggedCond::face() {
            FaceData::Union {
                documentation,
                arms,
            } => {
                assert_eq!(documentation, "Tagged union of inline and tuple arms.");
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].name, "EnvEq");
            }
            _ => panic!("tagged expected union data for union"),
        }
    }

    #[test]
    fn literal_data_showed_quoted_faces() {
        match Shade::face() {
            FaceData::Union {
                documentation,
                arms,
            } => {
                assert_eq!(documentation, "String face for data tests.");
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].face.as_ref(), "\"dark\"");
                assert_eq!(arms[1].face.as_ref(), "\"light\"");
            }
            _ => panic!("expected union data for literal"),
        }
    }

    #[test]
    fn callback_data_listed_params_without_returns() {
        match CallArgs::face() {
            FaceData::Function {
                documentation,
                parameters,
                returns,
            } => {
                assert_eq!(documentation, "Callback shape for data tests.");
                assert_eq!(parameters.len(), 2);
                assert_eq!(parameters[0].name, "route");
                assert_eq!(parameters[1].name, "document");
                assert_eq!(parameters[1].face.as_ref(), "LiveDoc");
                assert!(returns.is_empty());
            }
            _ => panic!("expected function data for callback shape"),
        }
    }

    #[test]
    fn userdata_data_kept_docs() {
        match Handle::face() {
            FaceData::Userdata { documentation } => {
                assert_eq!(documentation, "Userdata handle for data tests.");
            }
            _ => panic!("expected userdata data for userdata"),
        }
    }

    #[test]
    fn scalar_data_marks_primitives() {
        assert!(matches!(String::face(), FaceData::Scalar { .. }));
        assert!(matches!(bool::face(), FaceData::Scalar { .. }));
        assert!(matches!(Vec::<String>::face(), FaceData::Scalar { .. }));
        assert!(matches!(Option::<String>::face(), FaceData::Scalar { .. }));
    }

    #[test]
    fn skipped_union_data_keeps_all_arms() {
        match SkippedUnion::face() {
            FaceData::Union { arms, .. } => {
                assert_eq!(arms.len(), 2);
                assert_eq!(arms[0].name, "Live");
                assert_eq!(arms[1].name, "Sealed");
                assert_eq!(arms[0].fields.len(), 1);
            }
            _ => panic!("skipped expected union data for union"),
        }
    }

    #[test]
    fn flattened_holder_data_kept_outer_fields() {
        match FlatHolder::face() {
            FaceData::Class { fields, .. } => {
                assert_eq!(fields.len(), 1);
                assert_eq!(fields[0].name, "path");
                assert_eq!(fields[0].face.as_ref(), "string");
            }
            _ => panic!("flat expected class data for holder"),
        }
    }

    #[test]
    fn empty_cases_hold_no_fields_or_arms() {
        assert!(matches!(String::face(), FaceData::Scalar { .. }));
        assert!(matches!(CallArgs::face(), FaceData::Function { .. }));
        assert!(matches!(Handle::face(), FaceData::Userdata { .. }));
        match CallArgs::face() {
            FaceData::Function { returns, .. } => assert!(returns.is_empty()),
            _ => panic!("expected function data for callback"),
        }
        match LiveDoc::face() {
            FaceData::Class { fields, .. } => assert_eq!(fields.len(), 1),
            _ => panic!("expected class data for live doc"),
        }
    }
}
