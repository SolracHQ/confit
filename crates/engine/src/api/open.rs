//! Opens project files as handles.

use mlua::Lua;

use lua_binding::lua_module;

use crate::api::resource::ResourceHandle;
use crate::api::stores_from;

pub struct Open;

#[lua_module("confit.open")]
impl Open {
    /// Opens one project file.
    ///
    /// # Arguments
    ///
    /// - [path] Project-relative file path.
    ///
    /// # Returns
    ///
    /// - The resource handle, else nil.
    /// - The failure reason, else nil.
    pub fn open(lua: &Lua, path: String) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.resources().resource(&path) {
            Ok(found) => Ok((Some(ResourceHandle::new(found)), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    use crate::api::builtin;
    use confit_store::{StoreRoots, Stores};
    use mlua::Value;

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

    fn fresh_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let project = builtin(&lua, "confit.open").unwrap();
        lua.globals().set("open", project).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    fn project_root(dir: &Path) -> PathBuf {
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        project
    }

    #[test]
    fn open_reads_project_file_text() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("note.txt"), b"hello").unwrap();
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle = assert(open("note.txt"))
            return assert(handle:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("expected a string"),
            Err(error) => panic!("the open module reads the file: {error}"),
        }
    }

    #[test]
    fn open_returns_nil_for_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = fresh_lua(&stores);
        for segment in ["../outside", "/etc/passwd"] {
            let source = format!(
                "local handle, reason = open({segment:?}) \
                 assert(handle == nil) \
                 return reason"
            );
            match run(&lua, &source) {
                Ok(Value::String(_)) => {}
                Ok(_) => panic!("expected a string reason: {segment}"),
                Err(error) => panic!("the open module keeps the escape soft: {error}"),
            }
        }
    }

    #[test]
    fn open_returns_nil_for_an_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle, reason = open("absent.txt")
            assert(handle == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                assert!(
                    !reason.to_string_lossy().is_empty(),
                    "the reason shows the miss"
                )
            }
            Ok(_) => panic!("expected a string reason"),
            Err(error) => panic!("the open module keeps the miss soft: {error}"),
        }
    }

    #[test]
    fn open_rejects_non_text_paths() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = fresh_lua(&stores);
        let source = r#"return open(1)"#;
        assert!(
            run(&lua, source).is_err(),
            "the open module rejects a non-text path"
        );
    }

    #[test]
    fn structured_load_decodes_every_format() {
        for (name, body, script) in [
            (
                "payload.txt",
                "title = \"starship\"\n[settings]\nenabled = true\n",
                r#"
            local handle = assert(open("payload.txt"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.title == "starship")
            return found.settings.enabled
        "#,
            ),
            (
                "payload.toml",
                "{\"name\": \"starship\", \"enabled\": true}",
                r#"
            local handle = assert(open("payload.toml"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.name == "starship")
            return found.enabled
        "#,
            ),
            (
                "payload.json",
                "name: starship\nenabled: true\n",
                r#"
            local handle = assert(open("payload.json"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.name == "starship")
            return found.enabled
        "#,
            ),
            (
                "payload.yaml",
                "[server]\nhost = localhost\nport = 8080\n",
                r#"
            local handle = assert(open("payload.yaml"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.server.host == "localhost")
            return found.server.port
        "#,
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let stores = test_stores(dir.path());
            let project = project_root(dir.path());
            std::fs::write(project.join(name), body).unwrap();
            let lua = fresh_lua(&stores);
            assert!(
                run(&lua, script).is_ok(),
                "the structured loader decodes {name}"
            );
        }
    }

    #[test]
    fn structured_load_returns_nil_for_unknown_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("mystery.bin"), b"\x89PNG\r\n\x1a\nbinary").unwrap();
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle = assert(open("mystery.bin"))
            local found, reason = handle:load()
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("expected a string reason"),
            Err(error) => panic!("the structured loader keeps the failure soft: {error}"),
        }
    }

    #[test]
    fn deleted_file_load_matches_the_text_reason() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("gone.toml"), b"title = \"gone\"\n").unwrap();
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle = assert(open("gone.toml"))
            return handle
        "#;
        let handle = match run(&lua, source) {
            Ok(Value::UserData(found)) => found,
            Ok(_) => panic!("expected userdata"),
            Err(error) => panic!("the open module reads the file: {error}"),
        };
        lua.globals().set("saved", handle).unwrap();
        std::fs::remove_file(project.join("gone.toml")).unwrap();
        let source = r#"
            local found, reason = saved:load()
            assert(found == nil)
            local _, text_reason = saved:text()
            assert(reason == text_reason)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("expected a string reason"),
            Err(error) => panic!("the structured loader keeps a deleted file soft: {error}"),
        }
    }
}
