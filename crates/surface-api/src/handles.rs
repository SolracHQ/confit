//! Handles
//! Opened resource values shared by every opener.

use file_format::FileFormat;
use mlua::{Lua, LuaSerdeExt, Table, Value};
use mlua_extras::TypedUserData;
use mlua_extras::typeduserdata_impl;

use confit_store::Stores;
use confit_store::handles::ArchiveHandle as StoreArchive;
use confit_store::handles::ResourceHandle as StoreResource;
use confit_surface_face::proxy::StrictString;

/// Handle to an opened input.
#[derive(Clone, TypedUserData)]
pub struct Handle {
    /// The opened store identity.
    #[lua(skip)]
    handle: StoreResource,
    /// Store access for resource reads.
    #[lua(skip)]
    stores: Stores,
}

/// Handle to a verified compressed archive.
#[derive(Clone, TypedUserData)]
pub struct Archive {
    /// The verified archive identity.
    #[lua(skip)]
    inner: StoreArchive,
    /// Store access for archive reads.
    #[lua(skip)]
    stores: Stores,
}

impl std::fmt::Debug for Handle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handle")
            .field("handle", &self.handle)
            .finish()
    }
}

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

impl Handle {
    /// Build one handle from opened identity with store access.
    pub fn new(handle: StoreResource, stores: Stores) -> Self {
        Self { handle, stores }
    }

    /// Return the opened store identity.
    pub fn handle(&self) -> &StoreResource {
        &self.handle
    }
}

impl std::fmt::Debug for Archive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Archive")
            .field("inner", &self.inner)
            .finish()
    }
}

impl PartialEq for Archive {
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl Archive {
    /// Build one archive from verified identity with store access.
    pub fn new(inner: StoreArchive, stores: Stores) -> Self {
        Self { inner, stores }
    }
}

confit_surface_face::handle_schema!(Handle);
confit_surface_face::handle_schema!(Archive);

#[typeduserdata_impl]
impl Handle {
    /// Read file text, else nil with reason.
    fn text(&self) -> mlua::Result<(Option<String>, Option<String>)> {
        match self.stores.resources().read_text(&self.handle) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Render the opened content hash.
    fn sha(&self) -> mlua::Result<String> {
        Ok(self.handle.sha().hex())
    }

    /// Read file content into its table, else nil with reason.
    ///
    /// - Inputs without a table shape produce nil with reason.
    fn load(&self, lua: &Lua) -> mlua::Result<(Option<Table>, Option<String>)> {
        match sniffed_bytes(&self.stores, &self.handle) {
            Ok(bytes) => match table_for(lua, &self.handle, &bytes) {
                Ok(table) => Ok((Some(table), None)),
                Err(reason) => Ok((None, Some(reason))),
            },
            Err(reason) => Ok((None, Some(reason))),
        }
    }

    /// Register the opened input as its archive, else nil with reason.
    ///
    /// - Non-archives produce nil with reason.
    fn archive(&self) -> mlua::Result<(Option<Archive>, Option<String>)> {
        match self.stores.archives().archive(&self.handle) {
            Ok(found) => Ok((Some(Archive::new(found, self.stores.clone())), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

#[typeduserdata_impl]
impl Archive {
    /// List the archive member names, else nil with reason.
    ///
    /// - Members are archive-relative with forward slashes.
    fn members(&self) -> mlua::Result<(Option<Vec<String>>, Option<String>)> {
        match self.stores.archives().members(&self.inner) {
            Ok(names) => Ok((Some(names), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Open one archive member as its handle, else nil with reason.
    ///
    /// - Names are archive-relative with forward slashes.
    fn member(&self, name: StrictString) -> mlua::Result<(Option<Handle>, Option<String>)> {
        match self.stores.archives().extract_member(&self.inner, &name.0) {
            Ok(found) => Ok((Some(Handle::new(found, self.stores.clone())), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

/// Read one handle fully into bytes.
///
/// - Binary inputs fail with the reason.
fn sniffed_bytes(stores: &Stores, handle: &StoreResource) -> Result<Vec<u8>, String> {
    use std::io::{Read as _, Seek as _};
    let path = handle.canonical().display().to_string();
    let mut stream = stores
        .resources()
        .open(handle)
        .map_err(|error| error.to_string())?;
    match FileFormat::from_reader(&mut stream) {
        Ok(FileFormat::PlainText) => {}
        Ok(format) => {
            return Err(format!(
                "cannot load '{path}': unsupported kind '{}'",
                format.name()
            ));
        }
        Err(error) => {
            return Err(confit_store::resources::error::ResourceError::from_io(
                handle.canonical(),
                error,
            )
            .to_string());
        }
    }
    if let Err(error) = stream.rewind() {
        return Err(confit_store::resources::error::ResourceError::from_io(
            handle.canonical(),
            error,
        )
        .to_string());
    }
    let mut bytes = Vec::new();
    match stream.read_to_end(&mut bytes) {
        Ok(_) => Ok(bytes),
        Err(error) => Err(confit_store::resources::error::ResourceError::from_io(
            handle.canonical(),
            error,
        )
        .to_string()),
    }
}
fn table_for(lua: &Lua, handle: &StoreResource, bytes: &[u8]) -> Result<Table, String> {
    let path = handle.canonical().display().to_string();
    let text = match std::str::from_utf8(bytes) {
        Ok(found) => found,
        Err(_) => return Err(format!("cannot load '{path}'")),
    };
    if let Some(value) = parse_json(text)
        .or_else(|| parse_toml(text))
        .or_else(|| parse_yaml(text))
        .or_else(|| parse_ini(text))
    {
        match lua.to_value(&value) {
            Ok(Value::Table(table)) => return Ok(table),
            _ => return Err(format!("cannot load '{path}'")),
        }
    }
    Err(format!("cannot load '{path}'"))
}

fn parse_json(text: &str) -> Option<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text).ok()? {
        serde_json::Value::Object(_) => serde_json::from_str(text).ok(),
        _ => None,
    }
}

fn parse_toml(text: &str) -> Option<serde_json::Value> {
    let found = toml::from_str::<toml::Value>(text).ok()?;
    match serde_json::to_value(&found).ok()? {
        serde_json::Value::Object(_) => serde_json::to_value(&found).ok(),
        _ => None,
    }
}

fn parse_yaml(text: &str) -> Option<serde_json::Value> {
    let found = noyalib::from_str::<noyalib::Value>(text).ok()?;
    match serde_json::to_value(&found).ok()? {
        serde_json::Value::Object(_) => serde_json::to_value(&found).ok(),
        _ => None,
    }
}

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
    use std::path::{Path, PathBuf};

    use confit_driver::fs::TestGuard;
    use confit_driver::progress::Event;
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

    fn test_stores_with_events(dir: &Path) -> (Stores, crossbeam_channel::Receiver<Event>) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        (
            Stores::new(
                StoreRoots {
                    config_base: dir.join("config"),
                    temp_base: dir.join("temp"),
                    project_root: dir.join("project"),
                },
                sender,
            ),
            receiver,
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

    fn tarball(dir: &Path, members: &[(&str, &[u8])]) -> Vec<u8> {
        let dest = dir.join("scratch.tar.gz");
        let framed: Vec<confit_driver::tar::BuildMember<'_>> = members
            .iter()
            .map(|(name, bytes)| confit_driver::tar::BuildMember {
                name: (*name).to_owned(),
                len: bytes.len() as u64,
                reader: Box::new(std::io::Cursor::new(*bytes)),
                mode: 0o644,
            })
            .collect();
        let build = |path: &Path| {
            let sink = confit_driver::fs::create(path)?;
            confit_driver::tar::build_gzipped(sink, framed, 6)
        };
        if let Err(error) = confit_driver::atomic_write(&dest, build) {
            panic!("fixture builds: {error}");
        }
        confit_driver::fs::read(&dest).unwrap()
    }

    fn zipball(members: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write as _;

        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        for (name, bytes) in members {
            writer
                .start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn tarball_round_trips_through_archive() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        confit_driver::fs::write(&project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("fonts.tar.gz"))
            local archive, archive_reason = handle:archive()
            assert(archive ~= nil, archive_reason)
            local members, members_reason = archive:members()
            assert(members ~= nil, members_reason)
            assert(#members == 2)
            assert(members[1] == "a.txt")
            assert(members[2] == "sub/b.txt")
            local first = assert(archive:member("a.txt"))
            assert(first:text() == "alpha")
            local second = assert(archive:member("sub/b.txt"))
            return assert(second:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "beta"),
            Ok(_) => panic!("tarball member returns its text"),
            Err(error) => panic!("tarball round trips: {error}"),
        }
    }

    #[test]
    fn zip_round_trips_through_archive() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = zipball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        confit_driver::fs::write(&project.join("fonts.zip"), &raw).unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("fonts.zip"))
            local archive, archive_reason = handle:archive()
            assert(archive ~= nil, archive_reason)
            local members, members_reason = archive:members()
            assert(members ~= nil, members_reason)
            assert(#members == 2)
            assert(members[1] == "a.txt")
            assert(members[2] == "sub/b.txt")
            local first = assert(archive:member("a.txt"))
            assert(first:text() == "alpha")
            local second = assert(archive:member("sub/b.txt"))
            return assert(second:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "beta"),
            Ok(_) => panic!("zip member returns its text"),
            Err(error) => panic!("zip round trips: {error}"),
        }
    }

    #[test]
    fn missing_member_names_member() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(dir.path(), &[("a.txt", b"alpha")]);
        confit_driver::fs::write(&project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("fonts.tar.gz"))
            local archive = assert(handle:archive())
            local found, reason = archive:member("absent.txt")
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                let text = reason.to_string_lossy();
                assert!(
                    text.contains("absent.txt"),
                    "the text shows the member: {text}"
                );
            }
            Ok(_) => panic!("missing member returns nil with reason"),
            Err(error) => panic!("missing members stay soft: {error}"),
        }
    }

    #[test]
    fn non_archive_reports_reason() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("note.txt"), b"plain text").unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("note.txt"))
            local found, reason = handle:archive()
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(reason)) => {
                let text = reason.to_string_lossy();
                assert!(
                    text.contains("not a compressed archive"),
                    "the text shows the reason: {text}"
                );
            }
            Ok(_) => panic!("non-archive returns nil with reason"),
            Err(error) => panic!("non-archives stay soft: {error}"),
        }
    }

    #[test]
    fn repeated_member_reads_reuse_unpack() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let (stores, receiver) = test_stores_with_events(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        confit_driver::fs::write(&project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = test_lua(&stores);
        let source = r#"
            local handle = assert(resource("fonts.tar.gz"))
            local archive = assert(handle:archive())
            local first = assert(archive:member("a.txt"))
            assert(first:text() == "alpha")
            local second = assert(archive:member("a.txt"))
            return assert(second:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "alpha"),
            Ok(_) => panic!("repeated member returns its text"),
            Err(error) => panic!("repeated members reuse the extract: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(events.len(), 1, "repeat unpack fires once");
        match &events[0] {
            Event::Unpacked { total, .. } => assert_eq!(*total, 2),
            other => panic!("repeat unpack announces itself: {other:?}"),
        }
    }
}
