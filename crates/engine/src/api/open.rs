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

    use confit_driver::fs::TestGuard;
    use confit_driver::sha::Sha;
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
        let project = crate::api::builtin(&lua, "confit.open").unwrap();
        lua.globals().set("open", project).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    fn project_root(dir: &Path) -> PathBuf {
        let project = dir.join("project");
        confit_driver::fs::create_dir_all(&project).unwrap();
        project
    }

    #[test]
    fn opens_files_reading_text_with_hash() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("note.txt"), b"hello").unwrap();
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
        let source = r#"
            local handle = assert(open("note.txt"))
            return handle:sha()
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => {
                assert_eq!(found.to_string_lossy(), Sha::hash(b"hello").hex())
            }
            Ok(_) => panic!("project hash returns a string"),
            Err(error) => panic!("project hashes its file: {error}"),
        }
    }

    #[test]
    fn escapes_raise_where_written() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        for (segment, reason) in [
            ("../outside", "outside"),
            ("/etc/passwd", "expected a relative path"),
        ] {
            let source = format!(
                "local handle, reason = open({segment:?}) \
                 assert(handle == nil) \
                 return reason"
            );
            match run(&lua, &source) {
                Ok(Value::String(found)) => {
                    let text = found.to_string_lossy();
                    assert!(
                        text.contains("escapes"),
                        "the text names the boundary: {text}"
                    );
                    assert!(text.contains(reason), "the text shows the fault: {text}");
                }
                Ok(_) => panic!("escaping read returns nil with reason: {segment}"),
                Err(error) => panic!("escaping reads stay soft: {error}"),
            }
        }
    }

    #[test]
    fn misses_return_nil_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
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
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        let source = r#"return open(1)"#;
        match run(&lua, source) {
            Ok(_) => panic!("foreign input passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("a string"), "the text shows the want: {text}");
            }
        }
    }

    #[test]
    fn loads_toml_ignoring_extension() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(
            &project.join("payload.txt"),
            b"title = \"starship\"\n[settings]\nenabled = true\n",
        )
        .unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("payload.txt"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.title == "starship")
            return found.settings.enabled
        "#;
        match run(&lua, source) {
            Ok(Value::Boolean(found)) => assert!(found),
            Ok(_) => panic!("toml load returns its table"),
            Err(error) => panic!("toml loads: {error}"),
        }
    }

    #[test]
    fn loads_json_ignoring_extension() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(
            &project.join("payload.toml"),
            br#"{"name": "starship", "enabled": true}"#,
        )
        .unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("payload.toml"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.name == "starship")
            return found.enabled
        "#;
        match run(&lua, source) {
            Ok(Value::Boolean(found)) => assert!(found),
            Ok(_) => panic!("json load returns its table"),
            Err(error) => panic!("json loads: {error}"),
        }
    }

    #[test]
    fn loads_yaml_ignoring_extension() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(
            &project.join("payload.json"),
            b"name: starship\nenabled: true\n",
        )
        .unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("payload.json"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.name == "starship")
            return found.enabled
        "#;
        match run(&lua, source) {
            Ok(Value::Boolean(found)) => assert!(found),
            Ok(_) => panic!("yaml load returns its table"),
            Err(error) => panic!("yaml loads: {error}"),
        }
    }

    #[test]
    fn loads_ini_ignoring_extension() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(
            &project.join("payload.yaml"),
            b"[server]\nhost = localhost\nport = 8080\n",
        )
        .unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("payload.yaml"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.server.host == "localhost")
            return found.server.port
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "8080"),
            Ok(_) => panic!("ini load returns its table"),
            Err(error) => panic!("ini loads: {error}"),
        }
    }

    #[test]
    fn loads_unknown_kind_showing_kind() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("mystery.bin"), b"\x89PNG\r\n\x1a\nbinary").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(open("mystery.bin"))
            local found, reason = handle:load()
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                let text = reason.to_string_lossy();
                assert!(
                    text.contains("mystery.bin"),
                    "the text shows the file: {text}"
                );
                assert!(
                    text.contains("Portable Network Graphics"),
                    "the text shows the kind: {text}"
                );
            }
            Ok(_) => panic!("unknown kind returns nil with reason"),
            Err(error) => panic!("unknown kinds stay soft: {error}"),
        }
    }

    #[test]
    fn loads_missing_files_as_text_does() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("gone.toml"), b"title = \"gone\"\n").unwrap();
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
        confit_driver::fs::remove_file(&project.join("gone.toml")).unwrap();
        let source = r#"
            local found, reason = saved:load()
            assert(found == nil)
            local _, text_reason = saved:text()
            assert(reason == text_reason)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                let text = reason.to_string_lossy();
                assert!(text.contains("missing file"), "keeps the miss: {text}");
            }
            Ok(_) => panic!("deleted load returns nil with reason"),
            Err(error) => panic!("deleted loads stay soft: {error}"),
        }
    }
}
