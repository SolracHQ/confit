//! Faces
//!
//! Schema faces for data crossing the Lua boundary. Schemars
//! schemas walk through the translator into stub faces, so adding
//! a field moves the stub with zero hand edits.

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

/// Derives one stub face from the schemars schema of a data shape.
pub fn face_of<T: JsonSchema>() -> Type {
    match serde_json::to_value(schemars::schema_for!(T)) {
        Ok(root) => translate(&root),
        Err(_) => Type::any(),
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
    if let Some(names) = node.get("enum").and_then(Json::as_array) {
        return Type::union(
            names
                .iter()
                .filter_map(|name| name.as_str().map(|text| Type::literal(text.to_owned()))),
        );
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(members) = node.get(key).and_then(Json::as_array) {
            return Type::union(members.iter().map(translate).collect::<Vec<_>>());
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
                    .map(|name| translate_type(name.as_str().unwrap_or_default()))
                    .collect::<Vec<_>>(),
            ),
            None => Type::any(),
        },
    }
}

/// Answers the face for one JSON type name.
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

/// Answers the JSON value face: null with scalars and tables.
///
/// Arrays ride tables, functions with threads and userdata never
/// cross into JSON.
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
            translate(&json!({"$ref": "#/$defs/TextBody"})),
            Type::named("TextBody".to_owned())
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
