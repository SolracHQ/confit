//! Resource files opened from the project root.

use std::path::{Component, Path, PathBuf};

use mlua::{Lua, Value};
use mlua_extras::typed::generator::DefinitionBuilder;

use confit_store::handles::ResourceHandle as StoreResource;
use confit_store::resources::error::ResourceError;
use confit_store::{Stores, resources::Resources};
use confit_surface_face::module::{Module, VerbDecl};

use crate::error::ApiError;
use crate::handles::{Archive, Handle};
use crate::paths::split_checked;

/// Build the resource function with its face for files under the project
/// root.
pub fn build(
    lua: &Lua,
    stores: &Stores,
    project: &Path,
) -> mlua::Result<(Value, DefinitionBuilder)> {
    let mut module = Module::start(lua, "Resource files opened from the project root.")?;
    module.register::<Handle>();
    module.register::<Archive>();
    let stores = stores.clone();
    let project = project.to_path_buf();
    module.verb::<String, (Option<Handle>, Option<String>), _>(
        lua,
        VerbDecl {
            name: "resource",
            documentation: "Opens one project-relative file into its handle.",
            params: &[("path", "Project-relative file path.")],
            rets: &[
                "The resource handle, else nil.",
                "The failure reason, else nil.",
            ],
        },
        move |_, value| resource_impl(&stores, &project, value),
    )?;
    let (table, face) = module.finish();
    Ok((Value::Function(table.get("resource")?), face))
}

/// Open one project-relative file into its resource handle.
///
/// Missing files take nil with reason.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string inputs.
/// - [`ApiError::Escape`] for escaping paths.
/// - [`ApiError::Open`] for open failures.
fn resource_impl(
    stores: &Stores,
    project: &Path,
    relative: String,
) -> mlua::Result<(Option<Handle>, Option<String>)> {
    const CALLER: &str = "confit.resource";
    let parts = split_checked(&relative, false).map_err(|fault| ApiError::Escape {
        caller: CALLER.to_owned(),
        input: relative.clone(),
        reason: fault.reason().to_owned(),
    })?;
    let full = join_project(project, &parts);
    if !under_project(project, &full) {
        return Err(ApiError::Escape {
            caller: CALLER.to_owned(),
            input: relative.clone(),
            reason: format!("'{relative}' is outside the project root"),
        }
        .into());
    }
    match open(&stores.resources(), &full) {
        Ok(handle) => Ok((Some(Handle::new(handle, stores.clone())), None)),
        Err(OpenOutcome::Soft { reason }) => Ok((None, Some(reason))),
        Err(OpenOutcome::Raise(error)) => Err(ApiError::Open(error).into()),
    }
}

enum OpenOutcome {
    Soft { reason: String },
    Raise(ResourceError),
}

/// Open one joined file.
///
/// Escapes raise and missing files take soft failure with reason.
fn open(resources: &Resources, full: &Path) -> std::result::Result<StoreResource, OpenOutcome> {
    match resources.resource(full) {
        Ok(handle) => Ok(handle),
        Err(error @ ResourceError::Escape { .. }) => Err(OpenOutcome::Raise(error)),
        Err(error) => Err(OpenOutcome::Soft {
            reason: error.to_string(),
        }),
    }
}

fn join_project(project: &Path, parts: &[String]) -> PathBuf {
    let mut full = project.to_path_buf();
    for part in parts {
        full.push(part);
    }
    full
}

fn under_project(project: &Path, full: &Path) -> bool {
    normalize(full).starts_with(normalize(project))
}

/// Normalize one path lexically.
///
/// - Current-dir steps drop.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            _ => out.push(component),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::fs::TestGuard;
    use confit_model::sha::Sha;
    use confit_store::{StoreRoots, Stores};

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
        let project = crate::api::builtin(&lua, "confit.resource").unwrap();
        lua.globals().set("resource", project).unwrap();
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
            local handle = assert(resource("note.txt"))
            return assert(handle:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("project text returns a string"),
            Err(error) => panic!("project opens its file: {error}"),
        }
        let source = r#"
            local handle = assert(resource("note.txt"))
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
            ("/etc/passwd", "must stay relative"),
        ] {
            let source = format!("return resource({segment:?})");
            match run(&lua, &source) {
                Ok(_) => panic!("escaping project read passes: {segment}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(reason), "the text shows the boundary: {text}");
                }
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
            local handle, reason = resource("absent.txt")
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
    fn coerced_inputs_return_nil_with_reason() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        project_root(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local handle, reason = resource(1)
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
            Ok(_) => panic!("coerced input returns nil with reason"),
            Err(error) => panic!("coerced inputs stay soft: {error}"),
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
            local handle = assert(resource("payload.txt"))
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
            local handle = assert(resource("payload.toml"))
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
            local handle = assert(resource("payload.json"))
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
            local handle = assert(resource("payload.yaml"))
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
    fn loads_unknown_kind_naming_kind() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("mystery.bin"), b"\x89PNG\r\n\x1a\nbinary").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("mystery.bin"))
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
            local handle = assert(resource("gone.toml"))
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
