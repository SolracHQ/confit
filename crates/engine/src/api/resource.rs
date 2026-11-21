//! Content handles for Lua profiles.

use confit_core::resource_ref::ResourceRef;
use file_format::FileFormat;
use lua_binding::{Shape, lua_methods};
use mlua::{Lua, Table};

use crate::api::archive::ArchiveHandle;
use crate::api::stores_from;

/// A handle to one resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Shape)]
#[shape(object)]
pub struct ResourceHandle {
    /// The data reference.
    reference: ResourceRef,
}

impl ResourceHandle {
    /// Wraps one content hash in a handle.
    pub fn new(reference: ResourceRef) -> Self {
        Self { reference }
    }
}

#[lua_methods]
impl ResourceHandle {
    /// Returns the data reference for this handle.
    ///
    /// # Returns
    ///
    /// - The data reference.
    pub fn r#ref(&self) -> ResourceRef {
        self.reference
    }

    /// Reads one file as text.
    ///
    /// # Returns
    ///
    /// - The decoded file text.
    /// - The failure reason.
    fn text(&self, lua: &Lua) -> mlua::Result<(Option<String>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.resources().read_text(&self.r#ref()) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Loads one file into its Lua table.
    ///
    /// # Returns
    ///
    /// - The decoded table.
    /// - The failure reason.
    fn load(&self, lua: &Lua) -> mlua::Result<(Option<Table>, Option<String>)> {
        let stores = stores_from(lua)?;
        let resources = stores.resources();
        let reference = self.r#ref();
        let label = resources
            .resolve(reference.sha())
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| reference.hex());
        let bytes = match resources.read_bytes(&reference) {
            Ok(found) => found,
            Err(error) => return Ok((None, Some(error.to_string()))),
        };
        match sniffed_bytes(&label, &bytes).and_then(|checked| table_for(lua, &label, &checked)) {
            Ok(found) => Ok((Some(found), None)),
            Err(reason) => Ok((None, Some(reason))),
        }
    }

    /// Returns the archive handle for one source.
    ///
    /// # Returns
    ///
    /// - The archive handle, else nil.
    /// - The failure reason, else nil.
    fn archive(&self, lua: &Lua) -> mlua::Result<(Option<ArchiveHandle>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.archives().archive(&self.r#ref()) {
            Ok(found) => Ok((Some(ArchiveHandle::new(found)), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

/// Reads one payload and checks its file format.
fn sniffed_bytes(path: &str, bytes: &[u8]) -> std::result::Result<Vec<u8>, String> {
    match FileFormat::from_reader(std::io::Cursor::new(bytes)) {
        Ok(FileFormat::PlainText) => Ok(bytes.to_vec()),
        Ok(format) => Err(format!(
            "the file '{path}' holds an unsupported kind '{}'",
            format.name()
        )),
        Err(error) => Err(format!("the file '{path}' fails the format check: {error}")),
    }
}

/// Decodes one payload into a Lua table.
fn table_for(lua: &Lua, path: &str, bytes: &[u8]) -> std::result::Result<Table, String> {
    use mlua::Value;
    let text = match std::str::from_utf8(bytes) {
        Ok(found) => found,
        Err(_) => return Err(format!("the file '{path}' holds no table")),
    };
    if let Some(value) = parse_json(text)
        .or_else(|| parse_toml(text))
        .or_else(|| parse_yaml(text))
        .or_else(|| parse_ini(text))
    {
        use mlua::LuaSerdeExt as _;
        match lua.to_value(&value) {
            Ok(Value::Table(table)) => return Ok(table),
            _ => return Err(format!("the file '{path}' holds no table")),
        }
    }
    Err(format!("the file '{path}' holds no table"))
}

/// Decodes one JSON object.
fn parse_json(text: &str) -> Option<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text).ok()? {
        serde_json::Value::Object(_) => serde_json::from_str(text).ok(),
        _ => None,
    }
}

/// Decodes one TOML object.
fn parse_toml(text: &str) -> Option<serde_json::Value> {
    let found = toml::from_str::<toml::Value>(text).ok()?;
    match serde_json::to_value(&found).ok()? {
        serde_json::Value::Object(_) => serde_json::to_value(&found).ok(),
        _ => None,
    }
}

/// Decodes one YAML object.
fn parse_yaml(text: &str) -> Option<serde_json::Value> {
    let found = noyalib::from_str::<noyalib::Value>(text).ok()?;
    match serde_json::to_value(&found).ok()? {
        serde_json::Value::Object(_) => serde_json::to_value(&found).ok(),
        _ => None,
    }
}

/// Decodes one INI object.
fn parse_ini(text: &str) -> Option<serde_json::Value> {
    match serini::from_str::<serde_json::Value>(text).ok()? {
        serde_json::Value::Object(mut map) => {
            if let Some(serde_json::Value::Object(root)) = map.remove("") {
                for (key, value) in root {
                    map.insert(key, value);
                }
            }
            if map.is_empty() {
                return None;
            }
            Some(serde_json::Value::Object(map))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::sha::Sha;
    use confit_store::{StoreRoots, Stores};
    use mlua::Value;
    use std::path::{Path, PathBuf};

    use crate::api::builtin;

    fn test_stores(dir: &Path) -> Stores {
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            confit_events::EventHandles::silent(),
        )
    }

    fn fresh_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let open = builtin(&lua, "confit.open").unwrap();
        lua.globals().set("open", open).unwrap();
        lua
    }

    fn project_root(dir: &Path) -> PathBuf {
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        project
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    #[test]
    fn handle_wraps_its_hash() {
        let sha = Sha::hash(b"confit handle fixture");
        let handle = ResourceHandle::new(ResourceRef::new(sha));
        assert_eq!(handle.r#ref().sha(), &sha);
        assert_eq!(handle, ResourceHandle::new(ResourceRef::new(sha)));
    }

    #[test]
    fn resource_text_reads_through_lua() {
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
            Ok(_) => panic!("expected a string for project text"),
            Err(error) => panic!("the project fails to load its text: {error}"),
        }
    }

    #[test]
    fn resource_load_decodes_toml_through_lua() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(
            project.join("payload.txt"),
            b"title = \"starship\"\n[settings]\nenabled = true\n",
        )
        .unwrap();
        let lua = fresh_lua(&stores);
        let source = r#"
            local handle = assert(open("payload.txt"))
            local found, reason = handle:load()
            assert(reason == nil, reason)
            assert(found.title == "starship")
            return found.settings.enabled
        "#;
        match run(&lua, source) {
            Ok(Value::Boolean(found)) => assert!(found),
            Ok(_) => panic!("expected a table for the toml load"),
            Err(error) => panic!("the toml fails to load: {error}"),
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
            Err(error) => panic!("the deleted file fails to report a reason: {error}"),
        }
    }
}
