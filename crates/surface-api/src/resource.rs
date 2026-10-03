//! Resource
//!
//! Resource files sealed under the project root. Sealing jails to
//! the root, missing files answer nil with reason, escapes raise.

use std::path::{Component, Path, PathBuf};

use mlua::{Lua, Value};
use mlua_extras::typed::generator::DefinitionBuilder;

use confit_store::handles::ResourceHandle as StoreResource;
use confit_store::resources::error::ResourceError;
use confit_store::{Stores, resources::Resources};
use confit_surface_face::module::{Module, VerbDecl};

use crate::error::ApiError;
use crate::handles::ResourceHandle;
use crate::paths::split_checked;

/// Builds the resource function with its face, sealing files under
/// the project root.
pub fn build(lua: &Lua, stores: &Stores, root: &Path) -> mlua::Result<(Value, DefinitionBuilder)> {
    let mut module = Module::start(lua, "Sealed resource files behind exec-rooted handles.")?;
    module.register::<ResourceHandle>();
    let stores = stores.clone();
    let root = root.to_path_buf();
    module.verb::<String, (Option<ResourceHandle>, Option<String>), _>(
        lua,
        VerbDecl {
            name: "resource",
            doc: "Seals one resource-relative file into its handle.",
            params: &[("path", "Resource-relative file path.")],
            rets: &[
                "The resource handle, else nil.",
                "The failure reason, else nil.",
            ],
        },
        move |_, value| resource_impl(&stores, &root, value),
    )?;
    let (table, face) = module.finish();
    Ok((Value::Function(table.get("resource")?), face))
}

/// Seals one resource-relative file into its resource handle.
///
/// Relative strings pass, escapes raise, missing files answer
/// nil with reason.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string inputs.
/// - [`ApiError::Escape`] for escaping paths.
/// - [`ApiError::Seal`] for seal failures.
fn resource_impl(
    stores: &Stores,
    root: &Path,
    rel: String,
) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
    const CALLER: &str = "confit.resource";
    let parts = split_checked(&rel, false).map_err(|fault| ApiError::Escape {
        caller: CALLER.to_owned(),
        input: rel.clone(),
        reason: fault.reason().to_owned(),
    })?;
    let full = join_root(root, &parts);
    if !under_root(root, &full) {
        return Err(ApiError::Escape {
            caller: CALLER.to_owned(),
            input: rel.clone(),
            reason: format!("'{rel}' escapes its root"),
        }
        .into());
    }
    match seal(&stores.resources(), &full) {
        Ok(handle) => Ok((Some(ResourceHandle::new(handle, stores.clone())), None)),
        Err(SealOutcome::Soft { reason }) => Ok((None, Some(reason))),
        Err(SealOutcome::Raise(error)) => Err(ApiError::Seal(error).into()),
    }
}

enum SealOutcome {
    Soft { reason: String },
    Raise(ResourceError),
}

/// Seals one joined file.
///
/// Escapes raise, missing files answer nil with reason.
fn seal(resources: &Resources, full: &Path) -> std::result::Result<StoreResource, SealOutcome> {
    match resources.resource(full) {
        Ok(handle) => Ok(handle),
        Err(error @ ResourceError::Escape { .. }) => Err(SealOutcome::Raise(error)),
        Err(error) => Err(SealOutcome::Soft {
            reason: error.to_string(),
        }),
    }
}

fn join_root(root: &Path, parts: &[String]) -> PathBuf {
    let mut full = root.to_path_buf();
    for part in parts {
        full.push(part);
    }
    full
}

fn under_root(root: &Path, full: &Path) -> bool {
    normalize(full).starts_with(normalize(root))
}

/// Normalizes one path lexically.
///
/// Current-dir steps drop.
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
                cache_base: dir.join("cache"),
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
        let root = dir.join("project");
        confit_driver::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn seals_files_reading_text_with_hash() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let root = project_root(dir.path());
        confit_driver::fs::write(&root.join("note.txt"), b"hello").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("note.txt"))
            return assert(handle:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("project text answers a string"),
            Err(error) => panic!("project seals its file: {error}"),
        }
        let source = r#"
            local handle = assert(resource("note.txt"))
            return handle:sha()
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => {
                assert_eq!(found.to_string_lossy(), Sha::hash(b"hello").hex())
            }
            Ok(_) => panic!("project hash answers a string"),
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
            ("../outside", "escape"),
            ("/etc/passwd", "must stay relative"),
        ] {
            let source = format!("return resource({segment:?})");
            match run(&lua, &source) {
                Ok(_) => panic!("escaping project read passes: {segment}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(reason), "names the jail: {text}");
                }
            }
        }
    }

    #[test]
    fn misses_answer_nil_with_reason() {
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
                    "reason names the miss"
                )
            }
            Ok(_) => panic!("missing file answers nil with reason"),
            Err(error) => panic!("missing files stay soft: {error}"),
        }
    }

    #[test]
    fn coerced_inputs_answer_nil_with_reason() {
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
                    "reason names the miss"
                )
            }
            Ok(_) => panic!("coerced input answers nil with reason"),
            Err(error) => panic!("coerced inputs stay soft: {error}"),
        }
    }
}
