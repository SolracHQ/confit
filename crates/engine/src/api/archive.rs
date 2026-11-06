//! Archive handles for Lua profiles.

use confit_core::resource_ref::ResourceRef;
use lua_binding::{Shape, lua_methods};
use mlua::Lua;

use crate::api::resource::ResourceHandle;
use crate::api::stores_from;

/// A handle to one compressed archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Shape)]
#[shape(object)]
pub struct ArchiveHandle {
    /// The archive hash; the path resolves per call.
    reference: ResourceRef,
}

impl ArchiveHandle {
    /// Wraps one archive hash in a handle.
    pub fn new(reference: ResourceRef) -> Self {
        Self { reference }
    }
}

#[lua_methods]
impl ArchiveHandle {
    /// Lists the member names in archive-relative form with forward slashes.
    ///
    /// # Returns
    ///
    /// - The member names, else nil.
    /// - The failure reason, else nil.
    fn members(&self, lua: &Lua) -> mlua::Result<(Option<Vec<String>>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.archives().members(&self.reference) {
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
    /// - The member handle, else nil.
    /// - The failure reason, else nil.
    fn member(
        &self,
        lua: &Lua,
        name: String,
    ) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_from(lua)?;
        match stores.archives().member(&self.reference, &name) {
            Ok(found) => Ok((Some(ResourceHandle::new(found)), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_driver::archive::{Algorithm, Builder, Level};
    use confit_events::ToUi;
    use confit_store::{StoreRoots, Stores};
    use crossbeam_channel::Receiver;
    use mlua::Value;
    use std::path::{Path, PathBuf};

    use crate::api::builtin;

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
    fn archive_tarball_round_trips_members() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        std::fs::write(project.join("fonts.tar.gz"), &raw).unwrap();
        let lua = fresh_lua(&stores);
        let source = r#"
            local input = assert(open("fonts.tar.gz"))
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
        let source = r#"
            local input = assert(open("fonts.zip"))
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
        let source = r#"
            local input = assert(open("fonts.tar.gz"))
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
        let source = r#"
            local input = assert(open("note.txt"))
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
        let source = r#"
            local input = assert(open("fonts.tar.gz"))
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
            Some(ToUi::ArchiveFinished { members, .. }) => {
                assert_eq!(*members, 2, "the repeated unpack counts every member")
            }
            other => panic!("expected a finished event: {other:?}"),
        }
    }
}
