//! Configs
//! Plain config decode into names with documents and patches.

use std::collections::HashMap;

use mlua::{FromLua, Lua, Table, Value};

use confit_surface_face::mismatch::received;

use crate::document::Document;
use crate::error::{ApiError, Result};
use crate::hook::decode_hook;
use crate::patch::{Patch, decode_patch};
use crate::paths::decode_route;
use confit_model::hook::Hook;
use confit_model::routes::Route;

/// One decoded config.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// The config name.
    pub name: String,
    /// Documents keyed by destination route.
    pub documents: HashMap<Route, Document>,
    /// Patches in profile return order.
    pub patches: Vec<Patch>,
    /// Hooks in profile return order.
    pub hooks: Vec<Hook>,
}

impl Config {
    /// Build one config from its name with documents, patches and hooks.
    pub fn new(
        name: String,
        documents: HashMap<Route, Document>,
        patches: Vec<Patch>,
        hooks: Vec<Hook>,
    ) -> Self {
        Self {
            name,
            documents,
            patches,
            hooks,
        }
    }
}

/// Decode one config table into its name with documents, patches and hooks.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped configs, destinations and bodies.
/// - [`ApiError::Lua`] for Lua read failures.
pub(crate) fn decode_config(lua: &Lua, value: &Value) -> Result<Config> {
    let table = match value {
        Value::Table(table) => table.clone(),
        _ => {
            return Err(ApiError::mismatch(
                "config",
                "a config table",
                received(value),
            ));
        }
    };
    let mut name = None;
    let mut documents_table = None;
    let mut patches_table = None;
    let mut hooks_table = None;
    for pair in table.pairs::<Value, Value>() {
        let (key, item) = pair?;
        match key {
            Value::String(text) => {
                let field = text.to_string_lossy();
                match field.as_ref() {
                    "name" => name = Some(read_name(&item)?),
                    "documents" => {
                        documents_table = Some(section_table(&item, "documents")?);
                    }
                    "patches" => {
                        patches_table = Some(section_table(&item, "patches")?);
                    }
                    "hooks" => {
                        hooks_table = Some(section_table(&item, "hooks")?);
                    }
                    _ => {
                        return Err(ApiError::mismatch(
                            "config key",
                            "one of 'name', 'documents', 'patches' or 'hooks'",
                            format!("'{field}'"),
                        ));
                    }
                }
            }
            _ => {
                return Err(ApiError::mismatch(
                    "config key",
                    "one of 'name', 'documents', 'patches' or 'hooks'",
                    received(&key),
                ));
            }
        }
    }
    let Some(name) = name else {
        return Err(ApiError::mismatch("config name", "a string name", "nil"));
    };
    Ok(Config {
        name: name.clone(),
        documents: decode_documents(lua, documents_table)?,
        patches: decode_patches(lua, patches_table, Some(&name))?,
        hooks: decode_hooks(lua, hooks_table)?,
    })
}

/// Decode one config name into its string.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string names.
fn read_name(value: &Value) -> Result<String> {
    match value {
        Value::String(found) => Ok(found.to_string_lossy()),
        _ => Err(ApiError::mismatch(
            "config name",
            "a string name",
            received(value),
        )),
    }
}

/// Decode one config section table.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-table sections.
fn section_table(item: &Value, field: &str) -> Result<Table> {
    match item {
        Value::Table(table) => Ok(table.clone()),
        _ => Err(ApiError::mismatch(
            format!("config {field}"),
            "a table",
            received(item),
        )),
    }
}

/// Decode one documents map into bodies keyed by destination.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for foreign destinations and bodies.
/// - [`ApiError::Lua`] for Lua read failures.
pub(crate) fn decode_documents(
    lua: &Lua,
    table: Option<Table>,
) -> Result<HashMap<Route, Document>> {
    let mut documents = HashMap::new();
    if let Some(table) = table {
        for pair in table.pairs::<Value, Value>() {
            let (key, item) = pair?;
            let destination = decode_route(&key, "document destination")?;
            let body = Document::from_lua(item, lua).map_err(crate::error::api_of)?;
            documents.insert(destination, body);
        }
    }
    Ok(documents)
}

/// Decode one patches list into patches in order under the config owner.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped patches.
/// - [`ApiError::Lua`] for Lua read failures.
fn decode_patches(lua: &Lua, table: Option<Table>, owner: Option<&str>) -> Result<Vec<Patch>> {
    let mut patches = Vec::new();
    if let Some(table) = table {
        let len = table.len()?;
        for index in 1..=len {
            let item: Value = table.get(index)?;
            patches.push(decode_patch(lua, &item, owner)?);
        }
    }
    Ok(patches)
}

/// Decode one hooks list into hooks in order.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped hooks.
/// - [`ApiError::Lua`] for Lua read failures.
fn decode_hooks(lua: &Lua, table: Option<Table>) -> Result<Vec<Hook>> {
    let mut hooks = Vec::new();
    if let Some(table) = table {
        let len = table.len()?;
        for index in 1..=len {
            let item: Value = table.get(index)?;
            hooks.push(decode_hook(lua, &item)?);
        }
    }
    Ok(hooks)
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::StructuredFormat;

    fn test_lua() -> Lua {
        Lua::new()
    }

    fn config_of(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    #[test]
    fn config_decodes_documents_with_patches() {
        let lua = test_lua();
        let source = r#"
            return {
                name = "tools/bat",
                documents = { [{ base = "home", relative = ".bashrc" }] = { text = "hi" } },
                patches = {
                    { target = { base = "home", relative = "config.json" }, format = "json", apply = function(d) return d end },
                },
            }
        "#;
        let value = config_of(&lua, source);
        match decode_config(&lua, &value) {
            Ok(decoded) => {
                assert_eq!(decoded.documents.len(), 1);
                let key = decoded.documents.keys().next().unwrap();
                assert_eq!(key.display(), "home('.bashrc')");
                assert_eq!(decoded.patches.len(), 1);
                let target = match &decoded.patches[0] {
                    Patch::Structured(found) => &found.target,
                    Patch::Rc(found) => &found.target,
                };
                let route = match target {
                    crate::patch::PatchTarget::Target(route) => route,
                    crate::patch::PatchTarget::Filter(_) => panic!("target means route"),
                };
                assert_eq!(route.display(), "home('config.json')");
                match &decoded.patches[0] {
                    Patch::Structured(found) => {
                        assert_eq!(found.format, StructuredFormat::Json);
                    }
                    Patch::Rc(_) => panic!("format presence means structured"),
                }
            }
            Err(error) => panic!("config decodes: {error}"),
        }
    }

    #[test]
    fn patches_keep_list_order() {
        let lua = test_lua();
        let source = r#"
            return {
                name = "tools/both",
                patches = {
                    { target = { base = "home", relative = "one.json" }, format = "json", apply = function(d) return d end },
                    { target = { base = "home", relative = "first.sh" }, apply = function(d) return d end },
                    { target = { base = "home", relative = "two.json" }, format = "json", apply = function(d) return d end },
                    { target = { base = "home", relative = "second.sh" }, apply = function(d) return d end },
                },
            }
        "#;
        let value = config_of(&lua, source);
        match decode_config(&lua, &value) {
            Ok(decoded) => {
                assert_eq!(decoded.patches.len(), 4);
                for (patch, want) in [
                    (&decoded.patches[0], "home('one.json')"),
                    (&decoded.patches[1], "home('first.sh')"),
                    (&decoded.patches[2], "home('two.json')"),
                    (&decoded.patches[3], "home('second.sh')"),
                ] {
                    let target = match patch {
                        Patch::Structured(found) => &found.target,
                        Patch::Rc(found) => &found.target,
                    };
                    let route = match target {
                        crate::patch::PatchTarget::Target(route) => route,
                        crate::patch::PatchTarget::Filter(_) => panic!("target means route"),
                    };
                    assert_eq!(route.display(), want);
                }
                assert!(matches!(decoded.patches[0], Patch::Structured(_)));
                assert!(matches!(decoded.patches[1], Patch::Rc(_)));
                assert!(matches!(decoded.patches[2], Patch::Structured(_)));
                assert!(matches!(decoded.patches[3], Patch::Rc(_)));
            }
            Err(error) => panic!("patches decode: {error}"),
        }
    }

    #[test]
    fn unknown_config_keys_fail_naming_key() {
        let lua = test_lua();
        let value = config_of(&lua, "return { nope = 1 }");
        match decode_config(&lua, &value) {
            Ok(_) => panic!("unknown config key passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("nope"), "the text shows the key: {text}");
            }
        }
    }

    #[test]
    fn patch_owners_ride_config_names() {
        let lua = test_lua();
        let named = config_of(
            &lua,
            r#"
            return {
                name = "tools/bat",
                patches = {
                    { target = { base = "home", relative = "config.json" }, format = "json", apply = function(d) return d end },
                    { target = { base = "home", relative = "first.sh" }, apply = function(d) return d end },
                },
            }
        "#,
        );
        match decode_config(&lua, &named) {
            Ok(decoded) => {
                assert_eq!(decoded.patches.len(), 2);
                for patch in &decoded.patches {
                    let owner = match patch {
                        Patch::Structured(found) => found.owner.as_deref(),
                        Patch::Rc(found) => found.owner.as_deref(),
                    };
                    assert_eq!(owner, Some("tools/bat"));
                }
            }
            Err(error) => panic!("named config decodes: {error}"),
        }
        let nameless = config_of(
            &lua,
            r#"
            return {
                patches = {
                    { target = { base = "home", relative = "config.json" }, format = "json", apply = function(d) return d end },
                },
            }
        "#,
        );
        match decode_config(&lua, &nameless) {
            Ok(_) => panic!("nameless config passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("config name"),
                    "the text shows the field: {text}"
                );
                assert!(
                    text.contains("a string name"),
                    "the text shows the want: {text}"
                );
                assert!(text.contains("nil"), "the text shows the received: {text}");
            }
        }
    }

    #[test]
    fn missing_names_fail_naming_field() {
        let lua = test_lua();
        for source in ["return {}", r#"return { documents = {} }"#] {
            let value = config_of(&lua, source);
            match decode_config(&lua, &value) {
                Ok(_) => panic!("nameless config passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(
                        text.contains("config name"),
                        "the text shows the field: {text}"
                    );
                    assert!(
                        text.contains("a string name"),
                        "the text shows the want: {text}"
                    );
                    assert!(text.contains("nil"), "the text shows the received: {text}");
                }
            }
        }
        let value = config_of(&lua, r#"return { name = 1 }"#);
        match decode_config(&lua, &value) {
            Ok(_) => panic!("numeric name passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("config name"),
                    "the text shows the field: {text}"
                );
                assert!(
                    text.contains("number"),
                    "the text shows the received: {text}"
                );
            }
        }
    }

    #[test]
    fn foreign_destinations_fail_naming_want() {
        let lua = test_lua();
        let value = config_of(
            &lua,
            r#"return { name = "tools/test", documents = { ["oops"] = { text = "hi" } } }"#,
        );
        match decode_config(&lua, &value) {
            Ok(_) => panic!("foreign destination passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("a route table"),
                    "the text shows the want: {text}"
                );
            }
        }
    }

    #[test]
    fn hooks_decode_with_defaults() {
        let lua = test_lua();
        let source = r#"
            return {
                name = "tools/hooks",
                hooks = {
                    { argv = { "mise", "install" } },
                    {
                        argv = { "mise", { base = "home", relative = "bin" } },
                        path = { { base = "home", relative = "bin" } },
                        requires = { in_path = { name = "git" } },
                        when = { env_eq = { key = "TERM", value = "xterm" } },
                        checks = { { in_path = { name = "mise" } } },
                        timeout = "90",
                    },
                },
            }
        "#;
        let value = config_of(&lua, source);
        match decode_config(&lua, &value) {
            Ok(decoded) => {
                assert_eq!(decoded.hooks.len(), 2);
                let bare = &decoded.hooks[0];
                assert_eq!(bare.argv.len(), 2);
                assert!(bare.path.is_empty(), "path defaults empty");
                assert_eq!(bare.requires, None);
                assert_eq!(bare.when, None);
                assert!(bare.checks.is_empty(), "checks default empty");
                assert_eq!(bare.timeout_secs, 600);
                let full = &decoded.hooks[1];
                assert_eq!(full.argv.len(), 2);
                assert_eq!(full.path.len(), 1);
                assert!(full.requires.is_some(), "requires decodes");
                assert!(full.when.is_some(), "when decodes");
                assert_eq!(full.checks.len(), 1);
                assert_eq!(full.timeout_secs, 90);
            }
            Err(error) => panic!("hooks decode: {error}"),
        }
    }

    #[test]
    fn foreign_hook_fields_fail_naming_fields() {
        let lua = test_lua();
        let cases = [
            (
                r#"return { name = "tools/hooks", hooks = { {} } }"#,
                "hook argv",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = 1 } } }"#,
                "hook argv",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = {} } } }"#,
                "hook argv",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = { 1 } } } }"#,
                "hook argv",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = { "a" }, timeout = 1 } } }"#,
                "hook timeout",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = { "a" }, timeout = "nope" } } }"#,
                "hook timeout",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = { "a" }, requires = 1 } } }"#,
                "hook requires",
            ),
            (
                r#"return { name = "tools/hooks", hooks = { { argv = { "a" }, checks = 1 } } }"#,
                "hook checks",
            ),
        ];
        for (source, want) in cases {
            let value = config_of(&lua, source);
            match decode_config(&lua, &value) {
                Ok(_) => panic!("foreign hook passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(want), "the text shows the field: {text}");
                }
            }
        }
    }

    #[test]
    fn unknown_hook_keys_fail_naming_key() {
        let lua = test_lua();
        let value = config_of(
            &lua,
            r#"return { name = "tools/hooks", hooks = { { argv = { "a" }, nope = 1 } } }"#,
        );
        match decode_config(&lua, &value) {
            Ok(_) => panic!("unknown hook key passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("nope"), "the text shows the key: {text}");
            }
        }
    }
}
