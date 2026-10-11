//! Api
//! Module installs with profile decoding over ready values.

use std::collections::HashMap;

use mlua::{Lua, Table, Value};

use confit_model::routes::Route;
use confit_store::Stores;

use confit_surface_face::mismatch::received;

use crate::config::Config;
use crate::config::{decode_config, decode_documents};
use crate::document::Document;
use crate::error::{ApiError, Result};

/// Build one builtin module value.
///
/// # Errors
///
/// - [`ApiError::UnknownModule`] for unknown names.
/// - [`mlua::Error`] for missing open inputs and module build failures.
pub fn builtin(lua: &Lua, name: &str) -> mlua::Result<Value> {
    match name {
        "confit.resource" => {
            let stores = with_stores(lua, |stores| stores.clone())?;
            let root = stores.resources().roots.project_root.clone();
            crate::resource::build(lua, &stores, &root).map(|(value, _)| value)
        }
        "confit.validate" => crate::validate::build(lua).map(|(value, _)| value),
        "confit.fetch" => {
            let stores = with_stores(lua, |stores| stores.clone())?;
            crate::fetch::build(lua, &stores).map(|(value, _)| value)
        }
        "confit.version" => Ok(crate::version::value()),
        _ => Err(ApiError::UnknownModule {
            name: name.to_owned(),
        }
        .into()),
    }
}

/// One profile.
#[derive(Debug, Clone, Default)]
pub struct Profile {
    /// Shell names in profile order.
    pub shells: Vec<String>,
    /// Configs in profile order.
    pub configs: Vec<Config>,
    /// Documents keyed by destination route.
    pub documents: HashMap<Route, Document>,
}

impl Profile {
    /// Build one profile from shells with configs and documents.
    pub fn new(
        shells: Vec<String>,
        configs: Vec<Config>,
        documents: HashMap<Route, Document>,
    ) -> Self {
        Self {
            shells,
            configs,
            documents,
        }
    }

    /// Decode one profile table into shells with configs and documents.
    ///
    /// - Missing sections are empty.
    ///
    /// # Errors
    ///
    /// - [`ApiError::Mismatch`] for misshaped profiles, shells, configs
    ///   and documents.
    /// - [`ApiError::Lua`] for Lua read failures.
    pub fn decode(lua: &Lua, value: Value) -> Result<Self> {
        let table = match value {
            Value::Table(table) => table,
            _ => return Err(mismatch_return(&value)),
        };
        let mut shells_table = None;
        let mut configs_table = None;
        let mut documents_table = None;
        for pair in table.pairs::<Value, Value>() {
            let (key, item) = pair?;
            match key {
                Value::String(text) => {
                    let field = text.to_string_lossy();
                    match field.as_ref() {
                        "shells" => {
                            shells_table = Some(section_table(&item, "shells")?);
                        }
                        "configs" => {
                            configs_table = Some(section_table(&item, "configs")?);
                        }
                        "documents" => {
                            documents_table = Some(section_table(&item, "documents")?);
                        }
                        _ => {
                            return Err(ApiError::mismatch(
                                "profile key",
                                "one of 'shells', 'configs' or 'documents'",
                                format!("'{field}'"),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(ApiError::mismatch(
                        "profile key",
                        "one of 'shells', 'configs' or 'documents'",
                        received(&key),
                    ));
                }
            }
        }
        Ok(Self::new(
            decode_shells(shells_table)?,
            decode_configs(lua, configs_table)?,
            decode_documents(lua, documents_table)?,
        ))
    }
}

/// Decode one profile section table.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-table sections.
fn section_table(item: &Value, field: &str) -> Result<Table> {
    match item {
        Value::Table(table) => Ok(table.clone()),
        _ => Err(ApiError::mismatch(
            format!("profile {field}"),
            "a table",
            received(item),
        )),
    }
}

/// Decode one shells list into names in order.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string shells.
/// - [`ApiError::Lua`] for Lua read failures.
fn decode_shells(table: Option<Table>) -> Result<Vec<String>> {
    let mut shells = Vec::new();
    if let Some(table) = table {
        let len = table.len()?;
        for index in 1..=len {
            let item: Value = table.get(index)?;
            match item {
                Value::String(found) => shells.push(found.to_string_lossy()),
                _ => {
                    return Err(ApiError::mismatch(
                        "profile shell",
                        "a string",
                        received(&item),
                    ));
                }
            }
        }
    }
    Ok(shells)
}

/// Decode one configs list into configs in order.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped configs.
/// - [`ApiError::Lua`] for Lua read failures.
fn decode_configs(lua: &Lua, table: Option<Table>) -> Result<Vec<Config>> {
    let mut configs = Vec::new();
    if let Some(table) = table {
        let len = table.len()?;
        for index in 1..=len {
            let item: Value = table.get(index)?;
            configs.push(decode_config(lua, &item)?);
        }
    }
    Ok(configs)
}

fn mismatch_return(value: &Value) -> ApiError {
    ApiError::mismatch("profile", "a profile table", received(value))
}

/// Run one closure against the shared stores.
///
/// # Errors
///
/// - [`mlua::Error`] for an uninstalled api.
fn with_stores<T>(lua: &Lua, op: impl FnOnce(&Stores) -> T) -> mlua::Result<T> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(op(&stores)),
        None => Err(mlua::Error::RuntimeError(
            "confit api is not installed".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use confit_store::{StoreRoots, Stores};

    use crate::version::API_VERSION;

    const SINGLE: &str = r#"
        return {
            shells = { "bash" },
            configs = {
                {
                    name = "tools/bat",
                    documents = { [{ base = "home", relative = ".bashrc" }] = { text = "hi" } },
                },
            },
            documents = { [{ base = "home", relative = "top.txt" }] = { text = "top" } },
        }
    "#;
    const PAIR: &str = r#"
        return {
            configs = {
                {
                    name = "first",
                    documents = { [{ base = "home", relative = "zebra" }] = { text = "z" } },
                },
                {
                    name = "second",
                    documents = { [{ base = "home", relative = "apple" }] = { text = "a" } },
                },
            },
        }
    "#;

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.to_path_buf(),
            },
            sender,
        )
    }

    fn decoded(source: &str) -> Profile {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let value: Value = lua.load(source).call(()).unwrap();
        Profile::decode(&lua, value).unwrap()
    }

    #[test]
    fn single_profile_decodes_shells_with_configs_and_documents() {
        let found = decoded(SINGLE);
        assert_eq!(found.shells, ["bash".to_owned()]);
        assert_eq!(found.configs.len(), 1);
        let documents = &found.configs[0].documents;
        assert_eq!(documents.len(), 1);
        let (destination, body) = documents.iter().next().unwrap();
        assert_eq!(destination.display(), "home('.bashrc')");
        match body {
            crate::document::Document::Text(found) => assert_eq!(found.text, "hi"),
            _ => panic!("text body decodes"),
        }
        assert_eq!(found.documents.len(), 1);
        let (destination, _) = found.documents.iter().next().unwrap();
        assert_eq!(destination.display(), "home('top.txt')");
    }

    #[test]
    fn configs_decode_in_list_order() {
        let found = decoded(PAIR);
        assert_eq!(found.configs.len(), 2);
        for (config, want) in [(0, "home('zebra')"), (1, "home('apple')")] {
            let documents = &found.configs[config].documents;
            assert_eq!(documents.len(), 1);
            let (destination, _) = documents.iter().next().unwrap();
            assert_eq!(destination.display(), want);
        }
    }

    #[test]
    fn missing_sections_read_empty() {
        let found = decoded("return {}");
        assert!(found.shells.is_empty());
        assert!(found.configs.is_empty());
        assert!(found.documents.is_empty());
    }

    #[test]
    fn lists_keep_patches_per_config_in_return_order() {
        let source = r#"
            return {
                configs = {
                    {
                        name = "first",
                        patches = {
                            { target = { base = "home", relative = "one.json" }, format = "json", apply = function(d) return d end },
                        },
                    },
                    {
                        name = "second",
                        patches = {
                            { target = { base = "home", relative = "two.json" }, format = "json", apply = function(d) return d end },
                        },
                    },
                },
            }
        "#;
        let found = decoded(source);
        assert_eq!(found.configs.len(), 2);
        assert_eq!(found.configs[0].patches.len(), 1);
        let target = match &found.configs[0].patches[0] {
            crate::patch::Patch::Structured(found) => &found.target,
            crate::patch::Patch::Rc(found) => &found.target,
        };
        let route = match target {
            crate::patch::PatchTarget::Target(route) => route,
            crate::patch::PatchTarget::Filter(_) => panic!("target means route"),
        };
        assert_eq!(route.display(), "home('one.json')");
        assert_eq!(found.configs[1].patches.len(), 1);
        let target = match &found.configs[1].patches[0] {
            crate::patch::Patch::Structured(found) => &found.target,
            crate::patch::Patch::Rc(found) => &found.target,
        };
        let route = match target {
            crate::patch::PatchTarget::Target(route) => route,
            crate::patch::PatchTarget::Filter(_) => panic!("target means route"),
        };
        assert_eq!(route.display(), "home('two.json')");
    }

    #[test]
    fn repeated_names_decode_without_business_rules() {
        let source = r#"
            return {
                configs = {
                    { name = "same", documents = { [{ base = "home", relative = "a" }] = { text = "one" } } },
                    { name = "same", documents = { [{ base = "home", relative = "b" }] = { text = "two" } } },
                },
            }
        "#;
        let found = decoded(source);
        assert_eq!(found.configs.len(), 2);
        assert_eq!(found.configs[0].name, "same");
        assert_eq!(found.configs[1].name, "same");
    }

    #[test]
    fn lists_keep_both_patch_shapes_in_return_order() {
        let source = r#"
            return {
                configs = {
                    {
                        name = "first",
                        patches = {
                            { target = { base = "home", relative = "one.json" }, format = "json", apply = function(d) return d end },
                            { target = { base = "home", relative = "first.sh" }, apply = function(d) return d end },
                        },
                    },
                    {
                        name = "second",
                        patches = {
                            { target = { base = "home", relative = "two.json" }, format = "json", apply = function(d) return d end },
                            { target = { base = "home", relative = "second.sh" }, apply = function(d) return d end },
                        },
                    },
                },
            }
        "#;
        let found = decoded(source);
        assert_eq!(found.configs.len(), 2);
        assert_eq!(found.configs[0].patches.len(), 2);
        for (patch, want) in [
            (&found.configs[0].patches[0], "home('one.json')"),
            (&found.configs[0].patches[1], "home('first.sh')"),
            (&found.configs[1].patches[0], "home('two.json')"),
            (&found.configs[1].patches[1], "home('second.sh')"),
        ] {
            let target = match patch {
                crate::patch::Patch::Structured(found) => &found.target,
                crate::patch::Patch::Rc(found) => &found.target,
            };
            let route = match target {
                crate::patch::PatchTarget::Target(route) => route,
                crate::patch::PatchTarget::Filter(_) => panic!("target means route"),
            };
            assert_eq!(route.display(), want);
        }
    }

    #[test]
    fn unknown_profile_keys_fail_naming_key() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let value: Value = lua.load("return { nope = 1 }").call(()).unwrap();
        match Profile::decode(&lua, value) {
            Ok(_) => panic!("unknown profile key passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("nope"), "the text shows the key: {text}");
            }
        }
    }

    #[test]
    fn foreign_shells_fail_naming_want() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let value: Value = lua.load(r#"return { shells = { 1 } }"#).call(()).unwrap();
        match Profile::decode(&lua, value) {
            Ok(_) => panic!("foreign shell passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("a string"), "the text shows the want: {text}");
            }
        }
    }

    #[test]
    fn foreign_destinations_fail_naming_want() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let value: Value = lua
            .load(r#"return { documents = { ["oops"] = { text = "hi" } } }"#)
            .call(())
            .unwrap();
        match Profile::decode(&lua, value) {
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
    fn foreign_returns_fail_naming_want() {
        let lua = Lua::new();
        for (source, want) in [
            ("return 1", "a profile table"),
            ("return { nope = 1 }", "nope"),
        ] {
            let value: Value = lua.load(source).call(()).unwrap();
            match Profile::decode(&lua, value) {
                Ok(_) => panic!("foreign return passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(want), "the text shows the want: {text}");
                }
            }
        }
    }

    #[test]
    fn unknown_modules_fail() {
        let lua = Lua::new();
        match builtin(&lua, "confit.nope") {
            Ok(_) => panic!("unknown module passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("unknown module"),
                    "the text shows the miss: {text}"
                );
            }
        }
    }

    #[test]
    fn version_returns_one_integer() {
        let lua = Lua::new();
        match builtin(&lua, "confit.version") {
            Ok(Value::Integer(found)) => assert_eq!(found, API_VERSION),
            Ok(_) => panic!("version returns an integer"),
            Err(error) => panic!("version loads: {error}"),
        }
    }
}
