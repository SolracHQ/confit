//! Api
//!
//! Module installs with profile decoding over ready values.

use mlua::{Lua, Value};

use confit_store::Stores;

use confit_surface_face::mismatch::received;

use crate::declare::Profile;
use crate::error::{ApiError, Result};

/// Installs the shared stores behind the api.
///
/// Builds one builtin module value.
///
/// # Errors
///
/// - [`ApiError::UnknownModule`] for unknown names.
/// - [`mlua::Error`] for missing seal inputs and module build failures.
pub fn builtin(lua: &Lua, name: &str) -> mlua::Result<Value> {
    match name {
        "confit.path" => crate::paths::build(lua).map(|(value, _)| value),
        "confit.resource" => {
            let stores = with_stores(lua, |stores| stores.clone())?;
            let root = stores.resources().roots.project_root.clone();
            crate::resource::build(lua, &stores, &root).map(|(value, _)| value)
        }
        "confit.config" => crate::config::build(lua).map(|(value, _)| value),
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

/// Decodes one profile return value into owned declarations.
///
/// Collects every config in return order.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for missing returns and misshaped values.
pub fn decode_profile(value: Value) -> Result<Profile> {
    match &value {
        Value::UserData(data) => Ok(Profile::new(crate::config::declarations_of(data)?)),
        Value::Table(table) => {
            let mut documents = Vec::new();
            let len = table.len()?;
            for index in 1..=len {
                let item: Value = table.get(index)?;
                match &item {
                    Value::UserData(data) => documents.extend(
                        crate::config::declarations_of(data)
                            .map_err(|_| mismatch_at(index, &item))?,
                    ),
                    _ => {
                        return Err(mismatch_at(index, &item));
                    }
                }
            }
            Ok(Profile::new(documents))
        }
        _ => Err(mismatch_return(&value)),
    }
}

fn mismatch_at(index: i64, item: &Value) -> ApiError {
    ApiError::Mismatch {
        slot: format!("document {index}"),
        want: "a config value".to_owned(),
        received: received(item).to_owned(),
    }
}

fn mismatch_return(value: &Value) -> ApiError {
    ApiError::Mismatch {
        slot: "profile return".to_owned(),
        want: "a config or a list of configs".to_owned(),
        received: received(value).to_owned(),
    }
}

/// Runs one closure against the shared stores.
///
/// # Errors
///
/// - [`mlua::Error`] while the api stays uninstalled.
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
        local mine = config("tools/bat")
        mine:add_document(path.home(".bashrc"), { text = "hi" })
        return mine
    "#;
    const PAIR: &str = r#"
        local first = config("first")
        first:add_document(path.home("zebra"), { text = "z" })
        local second = config("second")
        second:add_document(path.home("apple"), { text = "a" })
        return { first, second }
    "#;

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

    fn decoded(source: &str) -> Profile {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let config = builtin(&lua, "confit.config").unwrap();
        let path = builtin(&lua, "confit.path").unwrap();
        lua.globals().set("config", config).unwrap();
        lua.globals().set("path", path).unwrap();
        let value: Value = lua.load(source).call(()).unwrap();
        decode_profile(value).unwrap()
    }

    #[test]
    fn single_documents_decode() {
        let found = decoded(SINGLE);
        assert_eq!(found.documents().len(), 1);
        assert_eq!(found.documents()[0].destination().display(), "home:.bashrc");
        match found.documents()[0].body() {
            crate::declare::Body::Text(found) => assert_eq!(found.text, "hi"),
            _ => panic!("text body decodes"),
        }
    }

    #[test]
    fn lists_decode_in_return_order() {
        let found = decoded(PAIR);
        assert_eq!(found.documents().len(), 2);
        assert_eq!(found.documents()[0].destination().display(), "home:zebra");
        assert_eq!(found.documents()[1].destination().display(), "home:apple");
    }

    #[test]
    fn foreign_returns_fail_naming_want() {
        let lua = Lua::new();
        for (source, want) in [
            ("return 1", "a config or a list of configs"),
            ("return { 1 }", "a config value"),
        ] {
            let value: Value = lua.load(source).call(()).unwrap();
            match decode_profile(value) {
                Ok(_) => panic!("foreign return passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(want), "names the want: {text}");
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
                assert!(text.contains("unknown module"), "names the miss: {text}");
            }
        }
    }

    #[test]
    fn version_answers_one_integer() {
        let lua = Lua::new();
        match builtin(&lua, "confit.version") {
            Ok(Value::Integer(found)) => assert_eq!(found, API_VERSION),
            Ok(_) => panic!("version answers an integer"),
            Err(error) => panic!("version answers: {error}"),
        }
    }
}
