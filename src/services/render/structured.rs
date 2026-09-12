//! Structured Rendering
//!
//! Table serializers for TOML, JSON, and YAML artifacts.

use crate::error::{Error, Result};
use crate::model::state::artifact::Table;

/// Renders a table to TOML text.
///
/// # Arguments
///
/// * `table` - the config values.
///
/// # Returns
///
/// TOML text. Empty tables yield the empty string.
///
/// # Errors
///
/// Serialization failures yield plan errors.
///
/// # Examples
/// ```rust
/// use confit::services::render::structured::render_toml;
///
/// let mut table = confit::model::state::artifact::Table::new();
/// table.insert("name".into(), serde_json::json!("bat"));
/// assert!(matches!(render_toml(&table), Ok(text) if text == "name = \"bat\"\n"));
/// ```
pub fn render_toml(table: &Table) -> Result<String> {
    toml::to_string(table).map_err(|e| Error::Plan(format!("render toml: {e}")))
}

/// Renders a table to pretty JSON text.
///
/// # Arguments
///
/// * `table` - the config values.
///
/// # Returns
///
/// Pretty JSON text.
///
/// # Errors
///
/// Serialization failures surface as JSON errors.
///
/// # Examples
/// ```rust
/// use confit::services::render::structured::render_json;
///
/// let table = confit::model::state::artifact::Table::new();
/// assert!(matches!(render_json(&table), Ok(text) if text == "{}"));
/// ```
pub fn render_json(table: &Table) -> Result<String> {
    Ok(serde_json::to_string_pretty(table)?)
}

/// Renders a table to YAML text.
///
/// # Arguments
///
/// * `table` - the config values.
///
/// # Returns
///
/// YAML text.
///
/// # Errors
///
/// Serialization failures yield plan errors.
///
/// # Examples
/// ```rust
/// use confit::services::render::structured::render_yaml;
///
/// let mut table = confit::model::state::artifact::Table::new();
/// table.insert("name".into(), serde_json::json!("bat"));
/// assert!(matches!(render_yaml(&table), Ok(text) if text == "name: bat"));
/// ```
pub fn render_yaml(table: &Table) -> Result<String> {
    noyalib::to_string(table).map_err(|e| Error::Plan(format!("render yaml: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(pairs: &[(&str, serde_json::Value)]) -> Table {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn toml_flat_table_matches_literal() {
        let rendered = render_toml(&table(&[
            ("name", serde_json::json!("bat")),
            ("version", serde_json::json!("latest")),
        ]))
        .unwrap();
        assert_eq!(rendered, "name = \"bat\"\nversion = \"latest\"\n");
    }

    #[test]
    fn json_flat_table_matches_literal() {
        let rendered = render_json(&table(&[("name", serde_json::json!("bat"))])).unwrap();
        assert_eq!(rendered, "{\n  \"name\": \"bat\"\n}");
    }

    #[test]
    fn yaml_flat_table_matches_literal() {
        let rendered = render_yaml(&table(&[("name", serde_json::json!("bat"))])).unwrap();
        assert_eq!(rendered, "name: bat");
    }

    #[test]
    fn empty_tables_render_minimally() {
        assert_eq!(render_toml(&Table::new()).unwrap(), "");
        assert_eq!(render_json(&Table::new()).unwrap(), "{}");
        assert_eq!(render_yaml(&Table::new()).unwrap(), "{}");
    }

    #[test]
    fn nested_tables_round_trip() {
        let data = table(&[(
            "tools",
            serde_json::json!({"bat": "latest", "list": [1, 2]}),
        )]);
        let toml_value: toml::Value = toml::from_str(&render_toml(&data).unwrap()).unwrap();
        assert_eq!(toml_value["tools"]["bat"].as_str(), Some("latest"));
        let json_value: serde_json::Value =
            serde_json::from_str(&render_json(&data).unwrap()).unwrap();
        assert_eq!(json_value["tools"]["bat"], serde_json::json!("latest"));
        let yaml_value: serde_json::Value =
            noyalib::from_str(&render_yaml(&data).unwrap()).unwrap();
        assert_eq!(yaml_value["tools"]["bat"], serde_json::json!("latest"));
    }
}
