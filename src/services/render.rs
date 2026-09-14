//! Render
//!
//! Data-to-bytes rendering for every document kind.

/// Shell rc text rendering with structural markers.
pub mod rc;
/// Bourne shell argument quoting.
pub mod shell_escape;
/// Table serializers for TOML, JSON, and YAML.
pub mod structured;

use std::collections::BTreeMap;

use crate::error::{Error, Result};
use crate::model::state::document::DocumentData;
use crate::model::state::document::StructuredFormat;
use crate::model::state::document::Table;
use crate::model::state::plan::Plan;

/// Renders text with minijinja, mapping syntax failures to plan errors.
///
/// # Arguments
///
/// * `text` - the template text under rendering.
/// * `facts` - the values feeding template slots.
/// * `prefix` - the error context naming the render site.
///
/// # Returns
///
/// Rendered text.
///
/// # Errors
///
/// Template syntax failures yield plan errors carrying the site prefix.
pub(crate) fn render_str(text: &str, facts: &Table, prefix: &str) -> Result<String> {
    minijinja::Environment::new()
        .render_str(text, facts)
        .map_err(|e| Error::Plan(format!("{prefix}{e}")))
}

/// Renders document data to on-disk bytes.
///
/// # Arguments
///
/// * `data` - the merged payload.
///
/// # Returns
///
/// Exact bytes landing on disk for the document.
///
/// # Errors
///
/// Structured syntax failures and TOML null values yield plan errors.
///
/// # Examples
/// ```rust
/// use confit::model::state::document::DocumentData;
/// use confit::services::render::render_document;
///
/// let data = DocumentData::Text { content: "hi".into() };
/// assert!(matches!(render_document(&data), Ok(_)));
/// ```
pub fn render_document(data: &DocumentData) -> Result<Vec<u8>> {
    match data {
        DocumentData::Structured { format, data } => match format {
            StructuredFormat::Toml => Ok(structured::render_toml(data)?.into_bytes()),
            StructuredFormat::Json => Ok(structured::render_json(data)?.into_bytes()),
            StructuredFormat::Yaml => Ok(structured::render_yaml(data)?.into_bytes()),
        },
        DocumentData::Text { content } => Ok(content.as_bytes().to_vec()),
        DocumentData::Link { target } => Ok(target.as_bytes().to_vec()),
        DocumentData::Rc(rc) => Ok(rc::render_rc(rc).into_bytes()),
    }
}

/// Renders baseline bytes for every planned document.
///
/// # Arguments
///
/// * `plan` - plan holding documents for rendering.
///
/// # Returns
///
/// Rendered bytes keyed by kind plus path.
///
/// # Errors
///
/// Fails with render errors from file reads.
pub fn render_baseline(plan: &Plan) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut rendered = BTreeMap::new();
    for document in &plan.documents {
        let key = document.key_string();
        let bytes = render_document(&document.data)?;
        rendered.insert(key, bytes);
    }
    Ok(rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::model::state::document::Table;

    fn table(pairs: &[(&str, serde_json::Value)]) -> Table {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn dispatch_covers_every_kind() {
        let structured = |format: StructuredFormat| DocumentData::Structured {
            format,
            data: table(&[("a", serde_json::json!(1))]),
        };
        let cases: Vec<(DocumentData, Vec<u8>)> = vec![
            (structured(StructuredFormat::Toml), b"a = 1\n".to_vec()),
            (
                structured(StructuredFormat::Json),
                b"{\n  \"a\": 1\n}".to_vec(),
            ),
            (structured(StructuredFormat::Yaml), b"a: 1".to_vec()),
            (
                DocumentData::Text {
                    content: "raw".into(),
                },
                b"raw".to_vec(),
            ),
            (
                DocumentData::Text {
                    content: String::new(),
                },
                Vec::new(),
            ),
            (
                DocumentData::Link {
                    target: "dest".into(),
                },
                b"dest".to_vec(),
            ),
            (
                DocumentData::Link {
                    target: String::new(),
                },
                Vec::new(),
            ),
        ];
        for (data, expected) in cases {
            assert_eq!(render_document(&data).unwrap(), expected);
        }
    }

    #[test]
    fn rc_dispatch_renders_generic_section() {
        let data = DocumentData::Rc(crate::model::state::rc::RcData {
            profile: Vec::new(),
            env: vec![crate::model::state::rc::EnvEntry {
                spec: crate::model::state::rc::EnvSpec {
                    name: "A".into(),
                    value: "1".into(),
                },
                when: None,
                priority: crate::model::state::level::Level::Normal,
            }],
            aliases: Vec::new(),
            init: Vec::new(),
        });
        assert_eq!(render_document(&data).unwrap(), b"export A=1\n");
    }

    #[test]
    fn syntax_error_is_plan_error() {
        let error = render_str("{{ unclosed", &table(&[]), "render init: ").unwrap_err();
        assert!(matches!(error, Error::Plan(_)), "{error}");
    }

    #[test]
    fn null_toml_value_is_plan_error() {
        let data = DocumentData::Structured {
            format: StructuredFormat::Toml,
            data: table(&[("a", serde_json::Value::Null)]),
        };
        let error = render_document(&data).unwrap_err();
        assert!(matches!(error, Error::Plan(_)), "{error}");
    }

    #[test]
    fn hash_matches_known_vectors() {
        assert_eq!(
            crate::security::sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            crate::security::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
