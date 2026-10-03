//! Config
//!
//! Named document collectors over plain add calls.

use mlua::{Lua, Value};
use mlua_extras::TypedUserData;
use mlua_extras::typed::generator::DefinitionBuilder;
use mlua_extras::typeduserdata_impl;

use confit_surface_face::module::{Module, VerbDecl};

use crate::declare::{Body, Declaration, SerializableValue, StructuredBody, TextBody};
use crate::error::{ApiError, Result};
use crate::paths::PathHandle;

/// Claim set holding taken config names per evaluate.
const SEEN_KEY: &str = "confit.config.names";

/// Document collector threading into the profile return.
#[derive(Debug, Clone, Default, TypedUserData)]
pub struct Config {
    /// The collector name.
    #[lua(skip)]
    name: String,
    /// Collected declarations in add order.
    #[lua(skip)]
    documents: Vec<Declaration>,
}

impl Config {
    /// Builds one empty collector under its name.
    pub fn new(name: String) -> Self {
        Self {
            name,
            documents: Vec::new(),
        }
    }

    /// Reads the collector name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reads the collected declarations in add order.
    pub fn documents(&self) -> &[Declaration] {
        &self.documents
    }
}

#[typeduserdata_impl]
impl Config {
    /// Records one document under its destination with its body.
    fn add_document(&mut self, dest: PathHandle, body: Body) -> mlua::Result<()> {
        self.documents
            .push(Declaration::new(dest.route().clone(), body));
        Ok(())
    }
}

/// Builds the config function with its face, answering collectors.
pub fn build(lua: &Lua) -> mlua::Result<(Value, DefinitionBuilder)> {
    let mut module = Module::start(lua, "Named document collectors.")?;
    module.register::<Config>();
    module.register::<SerializableValue>();
    module.register::<TextBody>();
    module.register::<StructuredBody>();
    module.verb::<String, Config, _>(
        lua,
        VerbDecl {
            name: "config",
            doc: "Builds one named collector.",
            params: &[("name", "Collector name.")],
            rets: &["The collector threading into the profile return."],
        },
        config_impl,
    )?;
    let (table, face) = module.finish();
    Ok((Value::Function(table.get("config")?), face))
}

/// Builds one named collector.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string and empty names.
/// - [`ApiError::Defined`] for repeated names.
fn config_impl(lua: &Lua, name: String) -> mlua::Result<Config> {
    if name.is_empty() {
        return Err(ApiError::Mismatch {
            slot: "name".to_owned(),
            want: "a non-empty name".to_owned(),
            received: "empty string".to_owned(),
        }
        .into());
    }
    claim_name(lua, &name)?;
    Ok(Config::new(name))
}

/// Claims one config name in the per-evaluate set.
///
/// # Errors
///
/// - [`ApiError::Defined`] for repeated names.
fn claim_name(lua: &Lua, name: &str) -> mlua::Result<()> {
    let seen: mlua::Table = match lua.named_registry_value(SEEN_KEY) {
        Ok(table) => table,
        Err(_) => {
            let table = lua.create_table()?;
            lua.set_named_registry_value(SEEN_KEY, table.clone())?;
            table
        }
    };
    let taken: Value = seen.get(name)?;
    if taken.is_nil() {
        seen.set(name, true)?;
        return Ok(());
    }
    Err(ApiError::Defined {
        name: name.to_owned(),
    }
    .into())
}

/// Clears the config name claim set.
pub fn reset(lua: &Lua) -> mlua::Result<()> {
    lua.set_named_registry_value(SEEN_KEY, lua.create_table()?)?;
    Ok(())
}

/// Reads owned declarations from one config handle.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for handles of another shape.
pub fn declarations_of(data: &mlua::AnyUserData) -> Result<Vec<Declaration>> {
    data.borrow::<Config>()
        .map(|found| found.documents().to_vec())
        .map_err(|_| ApiError::Mismatch {
            slot: "config".to_owned(),
            want: "a config value".to_owned(),
            received: "userdata".to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use confit_driver::fs::TestGuard;
    use confit_store::{StoreRoots, Stores};

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                cache_base: dir.join("cache"),
                temp_base: dir.join("temp"),
                project_root: dir.to_path_buf(),
            },
            sender,
        )
    }

    fn test_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        reset(&lua).unwrap();
        let config = crate::api::builtin(&lua, "confit.config").unwrap();
        let path = crate::api::builtin(&lua, "confit.path").unwrap();
        lua.globals().set("config", config).unwrap();
        lua.globals().set("path", path).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    fn collected(lua: &Lua, source: &str) -> Vec<Declaration> {
        match run(lua, source) {
            Ok(Value::UserData(data)) => match declarations_of(&data) {
                Ok(found) => found,
                Err(error) => panic!("config answers declarations: {error}"),
            },
            Ok(_) => panic!("config answers userdata"),
            Err(error) => panic!("collector builds: {error}"),
        }
    }

    #[test]
    fn collects_documents_in_add_order() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local mine = config("tools/bat")
            mine:add_document(path.home("zebra"), { text = "z" })
            mine:add_document(path.home("apple"), { text = "a" })
            return mine
        "#;
        let found = collected(&lua, source);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].destination().display(), "home:zebra");
        assert_eq!(found[1].destination().display(), "home:apple");
    }

    #[test]
    fn repeated_names_fail() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        match run(&lua, "config(\"x\") return config(\"x\")") {
            Ok(_) => panic!("repeated name passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("already defined"), "names the repeat: {text}");
                assert!(text.contains("'x'"), "names the name: {text}");
            }
        }
        reset(&lua).unwrap();
        match run(&lua, "return config(\"x\")") {
            Ok(_) => {}
            Err(error) => panic!("reset names pass again: {error}"),
        }
    }

    #[test]
    fn foreign_destinations_fail_naming_want() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local mine = config("tools/bat")
            return mine:add_document(".bashrc", { text = "hi" })
        "#;
        match run(&lua, source) {
            Ok(_) => panic!("string destination passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("a confit.path value"),
                    "names the want: {text}"
                );
                assert!(text.contains("string"), "names the received: {text}");
            }
        }
    }
}
