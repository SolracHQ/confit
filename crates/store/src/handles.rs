//! Unforgeable identity for trusted sources and pooled bytes.

use std::path::{Path, PathBuf};

use confit_core::sha::Sha;
use lua_binding::{Shape, lua_methods};
use mlua::{Lua, Table};
use serde::{Deserialize, Serialize};

use crate::Stores;

/// A handle to a trusted resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Shape)]
#[shape(object)]
pub enum ResourceHandle {
    /// A cache entry identified by its content hash.
    Cache(Sha),
    /// A project path with the identity hash taken at construction.
    Project(PathBuf, Sha),
}

impl Serialize for ResourceHandle {
    /// Writes the handle as its content hash.
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.sha().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResourceHandle {
    /// Decodes one handle from its content hash.
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let sha = Sha::deserialize(deserializer)?;
        Ok(Self::Cache(sha))
    }
}

impl ResourceHandle {
    /// Returns the content hash that identifies the handle.
    pub fn sha(&self) -> &Sha {
        match self {
            Self::Cache(sha) | Self::Project(_, sha) => sha,
        }
    }
}

/// A handle to a verified compressed archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Shape)]
#[shape(object)]
pub struct ArchiveHandle {
    sha: Sha,
    path: PathBuf,
}

impl ArchiveHandle {
    /// Returns the hash that identifies the archive.
    pub fn sha256(&self) -> &Sha {
        &self.sha
    }

    /// Returns the path of the archive file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Builds a handle from a verified archive hash and path.
    pub(crate) fn new(sha: Sha, path: PathBuf) -> Self {
        Self { sha, path }
    }
}

/// The path of one stored state file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateHandle {
    path: PathBuf,
}

impl StateHandle {
    /// Returns the path of the state file.
    pub fn canonical(&self) -> &Path {
        &self.path
    }

    /// Builds a handle for one state file path.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

#[lua_methods]
impl ResourceHandle {
    /// Reads one trusted file as text.
    ///
    /// # Returns
    ///
    /// - The decoded file text.
    /// - The failure reason.
    fn text(&self, lua: &Lua) -> mlua::Result<(Option<String>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.resources().read_text(self) {
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
        match stores.resources().load_table(lua, self) {
            Ok(found) => Ok((Some(found), None)),
            Err(reason) => Ok((None, Some(reason))),
        }
    }

    /// Returns the archive handle for one trusted source.
    ///
    /// # Returns
    ///
    /// - The archive handle.
    /// - The failure reason.
    fn archive(&self, lua: &Lua) -> mlua::Result<(Option<ArchiveHandle>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.archives().archive(self) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

#[lua_methods]
impl ArchiveHandle {
    /// Returns the archive member names in archive-relative form with forward slashes.
    ///
    /// # Returns
    ///
    /// - The member names.
    /// - The failure reason.
    fn members(&self, lua: &Lua) -> mlua::Result<(Option<Vec<String>>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.archives().members(self) {
            Ok(names) => Ok((Some(names), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Returns the handle for one archive member.
    ///
    /// # Arguments
    ///
    /// - [name] The member path with forward slashes.
    ///
    /// # Returns
    ///
    /// - The member handle.
    /// - The failure reason.
    fn member(
        &self,
        lua: &Lua,
        name: String,
    ) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.archives().member(self, &name) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

/// Returns the stores installed in the Lua app data.
///
/// # Errors
///
/// - [`mlua::Error`] when the api is not installed.
fn stores_from(lua: &Lua) -> mlua::Result<Stores> {
    match lua.app_data_ref::<Stores>() {
        Some(stores) => Ok(stores.clone()),
        None => Err(mlua::Error::RuntimeError(
            "the confit api is not installed".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreRoots;
    use confit_driver::archive::{Algorithm, Builder, Level};
    use confit_events::{ProgressEvent, ToUi};
    use crossbeam_channel::Receiver;
    use mlua::Value;

    fn fixture_sha() -> Sha {
        Sha::hash(b"confit handle fixture")
    }

    #[test]
    fn cache_handle_keeps_its_hash() {
        let handle = ResourceHandle::Cache(fixture_sha());
        assert_eq!(handle, ResourceHandle::Cache(fixture_sha()));
    }

    #[test]
    fn project_handle_keeps_its_path() {
        let sha = Sha::hash(b"starship");
        let handle = ResourceHandle::Project(PathBuf::from("/proj/starship.toml"), sha.clone());
        match &handle {
            ResourceHandle::Project(path, found) => {
                assert_eq!(path, Path::new("/proj/starship.toml"));
                assert_eq!(found, &sha);
            }
            ResourceHandle::Cache(_) => panic!("expected a project handle"),
        }
    }

    #[test]
    fn archive_handle_keeps_verified_identity() {
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

    fn test_stores_with_events(dir: &Path) -> (Stores, Receiver<ToUi>) {
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

    fn fresh_lua(stores: &Stores) -> Lua {
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
            Err(error) => panic!("the project fails to open: {error}"),
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
                panic!("the fixture fails to build: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("the fixture fails to finish: {error}"),
        }
    }

    fn zipball(members: &[(&str, &[u8])]) -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        for (name, bytes) in members {
            let mut reader = std::io::Cursor::new(*bytes);
            if let Err(error) = builder.member(name, 0o644, bytes.len() as u64, &mut reader) {
                panic!("the fixture fails to build: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("the fixture fails to finish: {error}"),
        }
    }

    #[test]
    fn resource_text_reads_through_lua() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("note.txt"), b"hello").unwrap();
        let lua = fresh_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "note.txt"))
            .unwrap();
        match run(&lua, "return assert(input:text())") {
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
            Ok(_) => panic!("expected a table for the toml load"),
            Err(error) => panic!("the toml fails to load: {error}"),
        }
    }

    #[test]
    fn archive_tarball_round_trips_members() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = fresh_lua(&stores);
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
            Ok(_) => panic!("expected a member text for the tarball"),
            Err(error) => panic!("the tarball fails to round trip: {error}"),
        }
    }

    #[test]
    fn archive_zip_round_trips_members() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = zipball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.zip"), &raw).unwrap();
        let lua = fresh_lua(&stores);
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
            Ok(_) => panic!("expected a member text for the zip"),
            Err(error) => panic!("the zip fails to round trip: {error}"),
        }
    }

    #[test]
    fn archive_member_reports_an_absent_name() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = fresh_lua(&stores);
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
            Ok(_) => panic!("expected a reason for an absent member"),
            Err(error) => panic!("the absent member fails to report a reason: {error}"),
        }
    }

    #[test]
    fn archive_open_reports_a_reason_for_text() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        std::fs::write(project.join("note.txt"), b"plain text").unwrap();
        let lua = fresh_lua(&stores);
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
            Ok(_) => panic!("expected a reason for a text file"),
            Err(error) => panic!("the text file fails to report a reason: {error}"),
        }
    }

    #[test]
    fn archive_member_reread_reuses_the_unpack() {
        let dir = tempfile::tempdir().unwrap();
        let (stores, receiver) = test_stores_with_events(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = fresh_lua(&stores);
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
            Ok(_) => panic!("expected a member text for the repeated member"),
            Err(error) => panic!("the repeated member fails to reuse the extract: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(events.len(), 2, "the repeated unpack writes two events");
        match events.last() {
            Some(ToUi::Progress(ProgressEvent::Finished { label, .. })) => assert!(
                label.contains("(2 members)"),
                "the repeated unpack counts every member: {label}"
            ),
            other => panic!("expected a finished event: {other:?}"),
        }
    }
}
