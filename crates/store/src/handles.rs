//! Handles
//! Unforgeable identity for trusted sources and pooled bytes.

use std::path::{Path, PathBuf};

use confit_binding_macros::{Face, face_impl};
use confit_driver::sha::Sha;
use mlua::{Lua, Table};
use serde::{Deserialize, Serialize};

use crate::Stores;

/// Handle to a project or cached resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Face)]
#[face(userdata)]
pub enum ResourceHandle {
    /// Content-addressed cache entry, trusted by its hash.
    Cache(Sha),
    /// Path plus identity sha, hashed once at construction, trusted after.
    Project(PathBuf, Sha),
}

impl Serialize for ResourceHandle {
    /// Serialize one handle as its content hash.
    ///
    /// - Project handles write the carried sha without reading the file.
    ///
    /// # Errors
    ///
    /// - Underlying serializer failures.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.sha().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResourceHandle {
    /// Deserialize one handle from its content hash.
    ///
    /// # Errors
    ///
    /// - Custom serde error for malformed hashes.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let sha = Sha::deserialize(deserializer)?;
        Ok(Self::Cache(sha))
    }
}

impl ResourceHandle {
    /// Return the trusted content hash.
    pub fn sha(&self) -> &Sha {
        match self {
            Self::Cache(sha) | Self::Project(_, sha) => sha,
        }
    }
}

/// Handle to a verified compressed archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Face)]
#[face(userdata)]
pub struct ArchiveHandle {
    sha: Sha,
    path: PathBuf,
}

impl ArchiveHandle {
    /// Return the content hash.
    pub fn sha256(&self) -> &Sha {
        &self.sha
    }

    /// Return the archive path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Build an archive handle from verified source identity.
    pub(crate) fn new(sha: Sha, path: PathBuf) -> Self {
        Self { sha, path }
    }
}

/// Handle to a stored state file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateHandle {
    path: PathBuf,
}

impl StateHandle {
    /// Return the state file path.
    pub fn canonical(&self) -> &Path {
        &self.path
    }

    /// Build a state handle from its file path.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[face_impl]
impl ResourceHandle {
    /// Read file text.
    ///
    /// # Returns
    ///
    /// - File text, else nil.
    /// - Failure reason, else nil.
    fn text(&self, lua: &Lua) -> mlua::Result<(Option<String>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.resources().read_text(self) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Read file content.
    ///
    /// # Returns
    ///
    /// - Decoded table, else nil.
    /// - Failure reason, else nil.
    fn load(&self, lua: &Lua) -> mlua::Result<(Option<Table>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.resources().load_table(lua, self) {
            Ok(found) => Ok((Some(found), None)),
            Err(reason) => Ok((None, Some(reason))),
        }
    }

    /// Open the input archive.
    ///
    /// # Returns
    ///
    /// - Archive handle, else nil.
    /// - Failure reason, else nil.
    fn archive(&self, lua: &Lua) -> mlua::Result<(Option<ArchiveHandle>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.archives().archive(self) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

#[face_impl]
impl ArchiveHandle {
    /// List the archive member names, archive-relative with forward slashes.
    ///
    /// # Returns
    ///
    /// - Member names, else nil.
    /// - Failure reason, else nil.
    fn members(&self, lua: &Lua) -> mlua::Result<(Option<Vec<String>>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.archives().members(self) {
            Ok(names) => Ok((Some(names), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Open one archive member.
    ///
    /// # Arguments
    ///
    /// - [name] Archive-relative member name.
    ///
    /// # Returns
    ///
    /// - Member handle, else nil.
    /// - Failure reason, else nil.
    fn member(
        &self,
        lua: &Lua,
        name: String,
    ) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.archives().member(self, &name) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

/// Read the shared stores from one Lua instance.
///
/// # Errors
///
/// - [`mlua::Error`] for an uninstalled api.
fn stores_of(lua: &Lua) -> mlua::Result<Stores> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.clone()),
        None => Err(mlua::Error::RuntimeError(
            "confit api is not installed".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreRoots;
    use confit_driver::archive::{Algorithm, Builder, Level};
    use confit_driver::progress::Event;
    use mlua::Value;

    fn fixture_sha() -> Sha {
        Sha::hash(b"confit handle fixture")
    }

    #[test]
    fn cache_handle_kept_hash_alone() {
        let handle = ResourceHandle::Cache(fixture_sha());
        assert_eq!(handle, ResourceHandle::Cache(fixture_sha()));
    }

    #[test]
    fn project_handle_kept_path_alone() {
        let sha = Sha::hash(b"starship");
        let handle = ResourceHandle::Project(PathBuf::from("/proj/starship.toml"), sha.clone());
        match &handle {
            ResourceHandle::Project(path, found) => {
                assert_eq!(path, Path::new("/proj/starship.toml"));
                assert_eq!(found, &sha);
            }
            ResourceHandle::Cache(_) => panic!("project keeps its path"),
        }
    }

    #[test]
    fn archive_handle_built_verified() {
        let handle = ArchiveHandle::new(fixture_sha(), PathBuf::from("archive.tar.gz"));
        assert_eq!(handle.sha256(), &fixture_sha());
        assert_eq!(handle.path(), Path::new("archive.tar.gz"));
    }

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
        lua
    }

    fn project_root(dir: &Path) -> PathBuf {
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        project
    }

    fn open_input(stores: &Stores, relative: &str) -> ResourceHandle {
        match stores.resources().resource(relative) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        }
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    fn tarball(members: &[(&str, &[u8])]) -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::TarGz, Level::default());
        for (name, bytes) in members {
            let mut reader = std::io::Cursor::new(*bytes);
            if let Err(error) = builder.member(name, 0o644, bytes.len() as u64, &mut reader) {
                panic!("fixture setup failed: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("fixture finishes: {error}"),
        }
    }

    fn zipball(members: &[(&str, &[u8])]) -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        for (name, bytes) in members {
            let mut reader = std::io::Cursor::new(*bytes);
            if let Err(error) = builder.member(name, 0o644, bytes.len() as u64, &mut reader) {
                panic!("fixture setup failed: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("fixture finishes: {error}"),
        }
    }

    #[test]
    fn parsed_text_with_hash_through_lua() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("note.txt"), b"hello").unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "note.txt"))
            .unwrap();
        match run(&lua, "return assert(input:text())") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("expected a string for project text"),
            Err(error) => panic!("project loads its text: {error}"),
        }
    }

    #[test]
    fn loads_toml_through_lua() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(
            project.join("payload.txt"),
            b"title = \"starship\"\n[settings]\nenabled = true\n",
        )
        .unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "payload.txt"))
            .unwrap();
        let source = r#"
            local found, reason = input:load()
            assert(reason == nil, reason)
            assert(found.title == "starship")
            return found.settings.enabled
        "#;
        match run(&lua, source) {
            Ok(Value::Boolean(found)) => assert!(found),
            Ok(_) => panic!("expected its table for toml load"),
            Err(error) => panic!("toml loads: {error}"),
        }
    }

    #[test]
    fn tarball_round_trips_through_archive() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "fonts.tar.gz"))
            .unwrap();
        let source = r#"
            local archive, archive_reason = input:archive()
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
            Ok(_) => panic!("expected its text for tarball member"),
            Err(error) => panic!("tarball round trips: {error}"),
        }
    }

    #[test]
    fn zip_round_trips_through_archive() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = zipball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.zip"), &raw).unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "fonts.zip"))
            .unwrap();
        let source = r#"
            local archive, archive_reason = input:archive()
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
            Ok(_) => panic!("expected its text for zip member"),
            Err(error) => panic!("zip round trips: {error}"),
        }
    }

    #[test]
    fn missing_member_names_member() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "fonts.tar.gz"))
            .unwrap();
        let source = r#"
            local archive = assert(input:archive())
            local found, reason = archive:member("absent.txt")
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("expected nil with reason for missing member"),
            Err(error) => panic!("missing members stay soft: {error}"),
        }
    }

    #[test]
    fn non_archive_reports_reason() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("note.txt"), b"plain text").unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "note.txt"))
            .unwrap();
        let source = r#"
            local found, reason = input:archive()
            assert(found == nil)
            return reason
        "#;
        match run(&lua, source) {
            Ok(Value::String(_)) => {}
            Ok(_) => panic!("expected nil with reason for non-archive"),
            Err(error) => panic!("non-archives stay soft: {error}"),
        }
    }

    #[test]
    fn member_reread_reuses_unpack() {
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let (stores, receiver) = test_stores_with_events(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "fonts.tar.gz"))
            .unwrap();
        let source = r#"
            local archive = assert(input:archive())
            local first = assert(archive:member("a.txt"))
            assert(first:text() == "alpha")
            local second = assert(archive:member("a.txt"))
            return assert(second:text())
        "#;
        match run(&lua, source) {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "alpha"),
            Ok(_) => panic!("expected its text for repeated member"),
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
