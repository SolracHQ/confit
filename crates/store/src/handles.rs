//! Handles
//! Unforgeable identity for trusted sources and pooled bytes.

use std::path::{Path, PathBuf};

use confit_binding_macros::{Face, face_impl};
use confit_driver::sha::Sha;
use mlua::{Lua, Table};
use serde::{Deserialize, Serialize};

use crate::Stores;
use crate::archive::error::ArchiveError;
use crate::resources::error::ResourceError;

/// Handle to a project resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Face)]
#[face(userdata)]
pub struct ResourceHandle {
    path: PathBuf,
    sha256: Sha,
}

/// Handle to pooled content bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BlobHandle {
    sha256: Sha,
    stored: Sha,
}

/// Content-addressed bytes ref for manifests.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobRef {
    /// Content hash for payload bytes.
    pub sha256: Sha,
    /// Pool-bytes hash for gzip bytes.
    pub stored: Sha,
}

/// Handle to a verified compressed archive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Face)]
#[face(userdata)]
pub struct ArchiveHandle {
    source_path: PathBuf,
    source_sha256: Sha,
}

impl ResourceHandle {
    /// Return the canonical source path.
    pub fn canonical(&self) -> &Path {
        &self.path
    }

    /// Return the content hash.
    pub fn sha256(&self) -> &Sha {
        &self.sha256
    }

    /// Build a resource handle from exec-rooted identity.
    ///
    /// # Errors
    ///
    /// - [`ResourceError::Escape`] for paths outside the exec root.
    pub(crate) fn new(
        exec_root: &Path,
        path: impl Into<PathBuf>,
        sha256: Sha,
    ) -> crate::resources::error::Result<Self> {
        let path = path.into();
        if !path.starts_with(exec_root) {
            return Err(ResourceError::Escape { path });
        }
        Ok(Self { path, sha256 })
    }
}

impl BlobHandle {
    /// Build a blob handle from verified content and stored hashes.
    pub(crate) fn new(sha256: Sha, stored: Sha) -> Self {
        Self { sha256, stored }
    }

    /// Return the content hash.
    pub fn sha256(&self) -> &Sha {
        &self.sha256
    }

    /// Return the pool-bytes hash.
    pub fn stored(&self) -> &Sha {
        &self.stored
    }

    /// Build the manifest ref with both hashes.
    pub fn to_ref(&self) -> BlobRef {
        BlobRef::new(self.sha256.clone(), self.stored.clone())
    }
}

impl BlobRef {
    /// Build one blob ref from verified content and stored hashes.
    pub fn new(sha256: Sha, stored: Sha) -> Self {
        Self { sha256, stored }
    }
}

impl ArchiveHandle {
    /// Return the canonical source path.
    pub fn canonical(&self) -> &Path {
        &self.source_path
    }

    /// Return the content hash.
    pub fn sha256(&self) -> &Sha {
        &self.source_sha256
    }

    /// Build an archive handle from verified source identity.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::NotArchive`] for empty source paths.
    pub(crate) fn new(
        source_path: impl Into<PathBuf>,
        source_sha256: Sha,
    ) -> crate::archive::error::Result<Self> {
        let source_path = source_path.into();
        if source_path.as_os_str().is_empty() {
            return Err(ArchiveError::NotArchive { path: source_path });
        }
        Ok(Self {
            source_path,
            source_sha256,
        })
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
    /// Read file text, else nil with reason.
    fn text(&self, lua: &Lua) -> mlua::Result<(Option<String>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.resources().read_text(self) {
            Ok(found) => Ok((Some(found), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Render the opened content hash.
    fn sha(&self) -> mlua::Result<String> {
        Ok(self.sha256.hex())
    }

    /// Read file content into its table, else nil with reason.
    ///
    /// - Inputs without a table shape produce nil with reason.
    fn load(&self, lua: &Lua) -> mlua::Result<(Option<Table>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.resources().load_table(lua, self) {
            Ok(found) => Ok((Some(found), None)),
            Err(reason) => Ok((None, Some(reason))),
        }
    }

    /// Open the input as its archive, else nil with reason.
    ///
    /// - Non-archives produce nil with reason.
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
    /// List the archive member names, else nil with reason.
    ///
    /// - Members are archive-relative with forward slashes.
    fn members(&self, lua: &Lua) -> mlua::Result<(Option<Vec<String>>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.archives().members(self) {
            Ok(names) => Ok((Some(names), None)),
            Err(error) => Ok((None, Some(error.to_string()))),
        }
    }

    /// Open one archive member as its handle, else nil with reason.
    ///
    /// - Names are archive-relative with forward slashes.
    fn member(
        &self,
        lua: &Lua,
        name: String,
    ) -> mlua::Result<(Option<ResourceHandle>, Option<String>)> {
        let stores = stores_of(lua)?;
        match stores.archives().extract_member(self, &name) {
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
    use confit_driver::sha::ShaError;
    use mlua::Value;

    fn fixture_sha() -> Sha {
        Sha::hash(b"confit handle fixture")
    }

    #[test]
    fn resource_handle_builds_under_exec_root() {
        let handle =
            ResourceHandle::new(Path::new("/proj"), "/proj/starship.toml", fixture_sha()).unwrap();
        assert_eq!(handle.canonical(), Path::new("/proj/starship.toml"));
        assert_eq!(handle.sha256(), &fixture_sha());
    }

    #[test]
    fn resource_handle_rejects_root_escape() {
        let error = match ResourceHandle::new(Path::new("/proj"), "/etc/passwd", fixture_sha()) {
            Ok(_) => panic!("escape passes"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            crate::resources::error::ResourceError::Escape { .. }
        ));
    }

    #[test]
    fn resource_handle_rejects_malformed_sha() {
        let error = match Sha::new("abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, ShaError::Parse { .. }));
    }

    #[test]
    fn blob_handle_builds_from_valid_sha() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha());
        assert_eq!(handle.sha256(), &fixture_sha());
    }

    #[test]
    fn blob_handle_rejects_malformed_sha() {
        for sha in ["abc", &"zz".repeat(32)] {
            let error = match Sha::new(sha) {
                Ok(_) => panic!("malformed sha passes"),
                Err(error) => error,
            };
            assert!(matches!(error, ShaError::Parse { .. }));
        }
    }

    #[test]
    fn blob_handle_converts_to_manifest_ref() {
        let handle = BlobHandle::new(fixture_sha(), fixture_sha());
        let blob = handle.to_ref();
        assert_eq!(blob.sha256, fixture_sha());
        assert_eq!(blob.stored, fixture_sha());
    }

    #[test]
    fn archive_handle_builds_verified() {
        let handle = ArchiveHandle::new("/cache/fonts.zip", fixture_sha()).unwrap();
        assert_eq!(handle.canonical(), Path::new("/cache/fonts.zip"));
        assert_eq!(handle.sha256(), &fixture_sha());
    }

    #[test]
    fn archive_handle_rejects_malformed_sha() {
        let error = match Sha::new("abc") {
            Ok(_) => panic!("malformed sha passes"),
            Err(error) => error,
        };
        assert!(matches!(error, ShaError::Parse { .. }));
    }

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            crate::StoreRoots {
                config_base: dir.join("config"),
                temp_base: dir.join("temp"),
                project_root: dir.join("project"),
            },
            sender,
        )
    }

    fn test_stores_with_events(
        dir: &Path,
    ) -> (
        Stores,
        crossbeam_channel::Receiver<confit_driver::progress::Event>,
    ) {
        let (sender, receiver) = crossbeam_channel::unbounded();
        (
            Stores::new(
                crate::StoreRoots {
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
        confit_driver::fs::create_dir_all(&project).unwrap();
        project
    }

    fn open_input(stores: &Stores, relative: &str) -> ResourceHandle {
        let path = stores.roots.project_root.join(relative);
        match stores.resources().resource(&path) {
            Ok(handle) => handle,
            Err(error) => panic!("project opens: {error}"),
        }
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
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
    fn reads_text_with_hash_through_lua() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("note.txt"), b"hello").unwrap();
        let lua = test_lua(&stores);
        lua.globals()
            .set("input", open_input(&stores, "note.txt"))
            .unwrap();
        match run(&lua, "return assert(input:text())") {
            Ok(Value::String(found)) => assert_eq!(found.to_string_lossy(), "hello"),
            Ok(_) => panic!("project text returns a string"),
            Err(error) => panic!("project reads its text: {error}"),
        }
        match run(&lua, "return input:sha()") {
            Ok(Value::String(found)) => {
                assert_eq!(found.to_string_lossy(), Sha::hash(b"hello").hex())
            }
            Ok(_) => panic!("project hash returns a string"),
            Err(error) => panic!("project hashes its file: {error}"),
        }
    }

    #[test]
    fn loads_toml_through_lua() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(
            &project.join("payload.txt"),
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
            Ok(_) => panic!("toml load returns its table"),
            Err(error) => panic!("toml loads: {error}"),
        }
    }

    #[test]
    fn tarball_round_trips_through_archive() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        confit_driver::fs::write(&project.join("fonts.tar.gz"), &raw).unwrap();
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
            Ok(_) => panic!("tarball member returns its text"),
            Err(error) => panic!("tarball round trips: {error}"),
        }
    }

    #[test]
    fn zip_round_trips_through_archive() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = zipball(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        confit_driver::fs::write(&project.join("fonts.zip"), &raw).unwrap();
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
            Ok(_) => panic!("zip member returns its text"),
            Err(error) => panic!("zip round trips: {error}"),
        }
    }

    #[test]
    fn missing_member_names_member() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(dir.path(), &[("a.txt", b"alpha")]);
        confit_driver::fs::write(&project.join("fonts.tar.gz"), &raw).unwrap();
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
        let _guard = confit_driver::fs::TestGuard::install();
        let stores = test_stores(dir.path());
        let project = project_root(dir.path());
        confit_driver::fs::write(&project.join("note.txt"), b"plain text").unwrap();
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
        use confit_driver::progress::Event;

        let dir = tempfile::tempdir().unwrap();
        let _guard = confit_driver::fs::TestGuard::install();
        let (stores, receiver) = test_stores_with_events(dir.path());
        let project = project_root(dir.path());
        let raw = tarball(dir.path(), &[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]);
        confit_driver::fs::write(&project.join("fonts.tar.gz"), &raw).unwrap();
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
