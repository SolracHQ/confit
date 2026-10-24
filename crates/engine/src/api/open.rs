//! Resource files opened from the project root.

use mlua::Lua;

use confit_binding_macros::face_impl;

use confit_store::handles::ResourceHandle;

use crate::api::stores_of;

/// Open namespace.
pub struct Open;

#[face_impl(namespace = "confit.open")]
impl Open {
    /// Open one project-relative file.
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
        let stores = stores_of(lua)?;
        match stores.resources().resource(&path) {
            Ok(handle) => Ok((Some(handle), None)),
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

    fn test_lua(stores: &Stores) -> Lua {
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
    fn opens_files_reading_text_with_hash() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("note.txt"), b"hello").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("note.txt"))
            return assert(handle:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("project text returns a string"),
            Err(error) => panic!("project opens its file: {error}"),
        }
    }

    #[test]
    fn escapes_raise_where_written() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        for segment in ["../outside", "/etc/passwd"] {
            let source = format!(
                "local handle, reason = open({segment:?}) \
                 assert(handle == nil) \
                 return reason"
            );
            match run(&lua, &source) {
                Ok(Value::String(_)) => {}
                Ok(_) => panic!("escaping read returns nil with reason: {segment}"),
                Err(error) => panic!("escaping load stays soft: {error}"),
            }
        }
    }

    #[test]
    fn misses_return_nil_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local handle, reason = open("absent.txt")
            assert(handle == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                assert!(
                    !reason.to_string_lossy().is_empty(),
                    "reason shows the miss"
                )
            }
            Ok(_) => panic!("missing file returns nil with reason"),
            Err(error) => panic!("missing files stay soft: {error}"),
        }
    }

    #[test]
    fn foreign_inputs_raise_showing_want() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        let source = r#"return open(1)"#;
        assert!(run(&lua, source).is_err(), "foreign input fails");
    }

    #[test]
    fn loads_structured_formats_ignoring_extension() {
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
            let lua = test_lua(&stores);
            assert!(run(&lua, script).is_ok(), "structured load serves {name}");
        }
    }

    #[test]
    fn loads_unknown_kind_showing_kind() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("mystery.bin"), b"\x89PNG\r\n\x1a\nbinary").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("mystery.bin"))
            local found, reason = handle:load()
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("unknown kind returns nil with reason"),
            Err(error) => panic!("unknown kinds stay soft: {error}"),
        }
    }

    #[test]
    fn loads_missing_files_as_text_does() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("gone.toml"), b"title = \"gone\"\n").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("gone.toml"))
            return handle
        "#;
        let handle = match run(&lua, source) {
            Ok(Value::UserData(found)) => found,
            Ok(_) => panic!("project opens its file"),
            Err(error) => panic!("project opens its file: {error}"),
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
            Ok(_) => panic!("deleted load returns nil with reason"),
            Err(error) => panic!("deleted loads stay soft: {error}"),
        }
    }
}
