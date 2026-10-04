//! Faces
//! Schema faces for data crossing the Lua boundary.

use schemars::JsonSchema;
use serde_json::Value as Json;

use mlua_extras::typed::{Index, Type};

/// Implements Typed through the schemars face of a data shape.
#[macro_export]
macro_rules! typed {
    ($target:ty) => {
        impl ::mlua_extras::typed::Typed for $target {
            fn ty() -> ::mlua_extras::typed::Type {
                $crate::translate::face_of::<Self>()
            }
        }
    };
}

/// Declares one sealed handle face under its type name.
///
/// Userdata ends schemas.
#[macro_export]
macro_rules! handle_schema {
    ($target:ty) => {
        impl ::schemars::JsonSchema for $target {
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                $crate::module::class_name::<$target>().into()
            }
            fn json_schema(_generator: &mut ::schemars::SchemaGenerator) -> ::schemars::Schema {
                let name = $crate::module::class_name::<$target>();
                ::schemars::Schema::new_ref(["#/$defs/", name.as_str()].concat())
            }
        }
    };
}

/// Derives one stub face from the schemars schema of a data shape.
pub fn face_of<T: JsonSchema>() -> Type {
    match serde_json::to_value(schemars::schema_for!(T)) {
        Ok(mut root) => {
            root_refs(&mut root, T::schema_name().as_ref());
            translate(&root)
        }
        Err(_) => Type::any(),
    }
}

/// Points recursive `#` self refs at the named root definition.
fn root_refs(node: &mut Json, name: &str) {
    match node {
        Json::Object(fields) => {
            if fields.get("$ref").and_then(Json::as_str) == Some("#") {
                fields.insert("$ref".to_owned(), Json::String(["#/$defs/", name].concat()));
            }
            for value in fields.values_mut() {
                root_refs(value, name);
            }
        }
        Json::Array(items) => {
            for item in items {
                root_refs(item, name);
            }
        }
        _ => {}
    }
}

/// Translates one schemars shape into its stub face.
pub fn translate(node: &Json) -> Type {
    if node.as_bool() == Some(true) {
        return json_value();
    }
    if let Some(name) = node.get("$ref").and_then(Json::as_str) {
        let bare = name.rsplit('/').next().unwrap_or(name);
        return Type::named(bare.to_owned());
    }
    if let Some(name) = node.get("const").and_then(Json::as_str) {
        return Type::literal(name.to_owned());
    }
    if let Some(names) = node.get("enum").and_then(Json::as_array) {
        return Type::union(
            names
                .iter()
                .filter_map(|name| name.as_str().map(|text| Type::literal(text.to_owned()))),
        );
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(members) = node.get(key).and_then(Json::as_array) {
            let faces: Vec<Type> = members.iter().map(translate).collect::<Vec<_>>();
            if let Some(extra) = sibling_fields(node, key) {
                return Type::union(
                    faces
                        .into_iter()
                        .map(|face| merge_fields(face, &extra))
                        .collect::<Vec<_>>(),
                );
            }
            return Type::union(faces);
        }
    }
    if let Some(members) = node.get("allOf").and_then(Json::as_array)
        && let [single] = members.as_slice()
    {
        return translate(single);
    }
    if node.get("type").and_then(Json::as_str) == Some("array") {
        return translate_array(node);
    }
    if node.get("type").and_then(Json::as_str) == Some("object") {
        return translate_object(node);
    }
    match node.get("type").and_then(Json::as_str) {
        Some(name) => translate_type(name),
        None => match node.get("type").and_then(Json::as_array) {
            Some(names) => Type::union(
                names
                    .iter()
                    .map(|name| match name.as_str() {
                        Some("array") => translate_array(node),
                        Some("object") => translate_object(node),
                        Some(single) => translate_type(single),
                        None => Type::any(),
                    })
                    .collect::<Vec<_>>(),
            ),
            None => Type::any(),
        },
    }
}

/// Returns the face for one JSON type name.
fn translate_type(name: &str) -> Type {
    match name {
        "string" => Type::string(),
        "integer" => Type::integer(),
        "number" => Type::number(),
        "boolean" => Type::boolean(),
        "null" => Type::nil(),
        _ => Type::any(),
    }
}

fn translate_array(node: &Json) -> Type {
    if let Some(prefix) = node.get("prefixItems").and_then(Json::as_array) {
        Type::tuple(prefix.iter().map(translate).collect::<Vec<_>>())
    } else if let Some(items) = node.get("items") {
        Type::array(translate(items))
    } else {
        Type::array(Type::any())
    }
}

/// Collects sibling object fields beside a composition key.
fn sibling_fields(node: &Json, composed: &str) -> Option<Vec<(Index, Type)>> {
    let mut sibling = node.clone();
    if let Some(object) = sibling.as_object_mut() {
        object.remove(composed);
        object.remove("anyOf");
        object.remove("oneOf");
    }
    match translate(&sibling) {
        Type::Table(fields) => {
            if fields.is_empty() {
                None
            } else {
                Some(fields.into_iter().collect())
            }
        }
        _ => None,
    }
}

/// Merges sibling fields into one union member.
///
/// - Member fields win on clashes.
fn merge_fields(face: Type, extra: &[(Index, Type)]) -> Type {
    match face {
        Type::Table(mut fields) => {
            for (key, value) in extra {
                if !fields.contains_key(key) {
                    fields.insert(key.clone(), value.clone());
                }
            }
            Type::Table(fields)
        }
        _ => face,
    }
}
/// Returns the JSON value face holding null with scalars and tables.
pub fn json_value() -> Type {
    Type::union(vec![
        Type::nil(),
        Type::boolean(),
        Type::integer(),
        Type::number(),
        Type::string(),
        Type::Single("table".into()),
    ])
}

fn translate_object(node: &Json) -> Type {
    let required: Vec<&str> = node
        .get("required")
        .and_then(Json::as_array)
        .map(|names| names.iter().filter_map(Json::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    let fields = node
        .get("properties")
        .and_then(Json::as_object)
        .map(|props| {
            props
                .iter()
                .map(|(key, shape)| {
                    let face = translate(shape);
                    let face = if required.contains(&key.as_str()) {
                        face
                    } else {
                        face | Type::nil()
                    };
                    (Index::Str(key.clone().into()), face)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if fields.is_empty()
        && let Some(extra) = node.get("additionalProperties")
        && extra.as_bool() != Some(false)
    {
        return Type::map(Type::string(), translate(extra));
    }
    Type::table(fields)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(JsonSchema)]
    struct Named {
        #[allow(dead_code)]
        text: String,
    }

    #[test]
    fn translate_names_scalar_shapes() {
        assert_eq!(translate(&json!({"type": "string"})), Type::string());
        assert_eq!(translate(&json!({"type": "integer"})), Type::integer());
        assert_eq!(translate(&json!({"type": "boolean"})), Type::boolean());
    }

    #[test]
    fn translate_unions_const_members() {
        assert_eq!(
            translate(&json!({"const": "json"})),
            Type::literal("json".to_owned())
        );
    }

    #[test]
    fn translate_unions_enums_with_refs() {
        assert_eq!(
            translate(&json!({"enum": ["toml", "json"]})),
            Type::union(vec![
                Type::literal("toml".to_owned()),
                Type::literal("json".to_owned())
            ])
        );
        assert_eq!(
            translate(&json!({"anyOf": [{"type": "string"}, {"type": "integer"}]})),
            Type::union(vec![Type::string(), Type::integer()])
        );
        assert_eq!(
            translate(&json!({"$ref": "#/$defs/TextDocument"})),
            Type::named("TextDocument".to_owned())
        );
    }

    #[test]
    fn face_names_root_self_refs() {
        #[derive(JsonSchema)]
        #[allow(dead_code)]
        enum Linked {
            End,
            More(Vec<Linked>),
        }
        let text = format!("{:?}", face_of::<Linked>());
        assert!(text.contains("Linked"), "names the root: {text}");
        assert!(
            !text.contains("\"#\""),
            "never leaks the root pointer: {text}"
        );
    }

    #[test]
    fn translate_maps_open_objects() {
        assert_eq!(
            translate(&json!({"type": "object", "additionalProperties": true})),
            Type::map(Type::string(), json_value())
        );
    }

    #[test]
    fn translate_unions_type_arrays() {
        assert_eq!(
            translate(&json!({"type": ["string", "null"]})),
            Type::union(vec![Type::string(), Type::nil()])
        );
    }

    #[test]
    fn translate_nills_optional_fields() {
        let face = translate(
            &json!({"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]}),
        );
        assert_eq!(
            face,
            Type::table(vec![(Index::Str("text".into()), Type::string())])
        );
        assert!(face_of::<Named>().is_table());
    }
}
