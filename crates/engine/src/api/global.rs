//! Bare constructor globals over model identities.

use mlua::Lua;

use confit_binding_macros::face_impl;

use confit_model::id::Id;
use confit_model::routes::{Route, RouteBase};

/// Global constructors.
pub struct Global;

#[face_impl(global)]
impl Global {
    /// Build one opaque id.
    ///
    /// # Arguments
    ///
    /// - [name] The id text.
    ///
    /// # Returns
    ///
    /// - The new id.
    #[allow(clippy::self_named_constructors)]
    pub fn id(lua: &Lua, name: String) -> mlua::Result<Id> {
        Ok(Id::traced(
            lua,
            Id {
                id: name,
                ..Default::default()
            },
        ))
    }

    /// Build one route.
    ///
    /// # Arguments
    ///
    /// - [base] The destination base.
    /// - [relative] The relative path.
    ///
    /// # Returns
    ///
    /// - The new route.
    #[allow(clippy::self_named_constructors)]
    pub fn route(lua: &Lua, base: RouteBase, relative: String) -> mlua::Result<Route> {
        let built =
            Route::new(base, relative).map_err(|cause| mlua::Error::external(cause.to_string()))?;
        Ok(Route::traced(lua, built))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use confit_binding::value::Decode;
    use confit_model::id::Id;
    use confit_store::{StoreRoots, Stores};
    use mlua::Value;

    use crate::engine::Engine;
    use crate::loader::{install, load_chunk};

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        )
    }

    fn project_root(dir: &Path) -> PathBuf {
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        lua.set_app_data(stores.clone());
        install(&lua).unwrap();
        lua
    }

    #[test]
    fn id_visible_in_chunk_nested_and_plugin_envs() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        std::fs::write(root.join("helper.lua"), b"return id(\"nested\")").unwrap();
        let entry = root.join("plugins").join("alice").join("tool");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(entry.join("init.lua"), b"return id(\"plugin\")").unwrap();
        let chunk = root.join("chunk.lua");
        std::fs::write(&chunk, b"return id(\"chunk\")").unwrap();
        let nested = root.join("nested.lua");
        std::fs::write(&nested, b"return require(\"helper\")").unwrap();
        let plugin = root.join("plugin.lua");
        std::fs::write(&plugin, b"return require(\"plugins.alice.tool\")").unwrap();
        let lua = test_lua(&stores);
        match load_chunk(&lua, &chunk) {
            Ok(Value::UserData(found)) => {
                let decoded = Id::decode(&Value::UserData(found), &lua).unwrap();
                assert_eq!(decoded.as_str(), "chunk");
            }
            Ok(_) => panic!("chunk id returns userdata"),
            Err(error) => panic!("chunk id loads: {error}"),
        }
        match load_chunk(&lua, &nested) {
            Ok(Value::UserData(found)) => {
                let decoded = Id::decode(&Value::UserData(found), &lua).unwrap();
                assert_eq!(decoded.as_str(), "nested");
            }
            Ok(_) => panic!("nested id returns userdata"),
            Err(error) => panic!("nested id loads: {error}"),
        }
        match load_chunk(&lua, &plugin) {
            Ok(Value::UserData(found)) => {
                let decoded = Id::decode(&Value::UserData(found), &lua).unwrap();
                assert_eq!(decoded.as_str(), "plugin");
            }
            Ok(_) => panic!("plugin id returns userdata"),
            Err(error) => panic!("plugin id loads: {error}"),
        }
    }

    #[test]
    fn id_capture_holds_file_line_and_plugin_chain() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        let entry = root.join("plugins").join("alice").join("capture");
        std::fs::create_dir_all(&entry).unwrap();
        std::fs::write(
            entry.join("init.lua"),
            b"return function() return id(\"captured\") end",
        )
        .unwrap();
        let chunk = root.join("test.lua");
        std::fs::write(
            &chunk,
            b"local make = require(\"plugins.alice.capture\")\nlocal myid = make()\nreturn myid",
        )
        .unwrap();
        let lua = test_lua(&stores);
        match load_chunk(&lua, &chunk) {
            Ok(Value::UserData(found)) => {
                let decoded = Id::decode(&Value::UserData(found), &lua).unwrap();
                assert_eq!(decoded.as_str(), "captured");
                let trace = decoded.trace.0.clone();
                assert!(trace.len() >= 2, "chain holds two frames");
                assert!(
                    trace[0].file.contains("init.lua"),
                    "first frame shows the plugin file"
                );
                assert!(
                    trace[0].line.parse::<usize>().unwrap_or(0) > 0,
                    "first frame shows the line"
                );
                assert!(
                    trace[1].file.contains("test"),
                    "second frame shows the chunk file"
                );
                assert!(
                    trace[1].line.parse::<usize>().unwrap_or(0) > 0,
                    "second frame shows the line"
                );
            }
            Ok(_) => panic!("captured id returns userdata"),
            Err(error) => panic!("captured id loads: {error}"),
        }
    }

    #[test]
    fn duplicate_same_ids_keep_terminal_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let (sender, _) = crossbeam_channel::unbounded();
        let stores = Stores::new(
            StoreRoots {
                config_base: dir.path().join("config"),
                temp_base: dir.path().join("temp"),
                project_root: dir.path().join("project"),
            },
            sender,
        );
        let root = stores.resources().roots.project_root.clone();
        std::fs::create_dir_all(&root).unwrap();
        let profile = root.join("profile.lua");
        std::fs::write(
            &profile,
            br#"
                return {
                    configs = {
                        { name = id("same"), documents = { { path = route("home", "a"), text = "one" } } },
                        { name = id("same"), documents = { { path = route("home", "b"), text = "two" } } },
                    },
                }
            "#,
        )
        .unwrap();
        let engine = Engine::new(stores);
        match engine.evaluate(&profile) {
            Ok(_) => panic!("duplicate ids pass"),
            Err(error) => assert_eq!(
                error.to_string(),
                "Cannot render the manifest, duplicate document 'same'"
            ),
        }
    }
}
