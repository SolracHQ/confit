//! Member listing and extraction for compressed archives.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_core::error::{
    ArchiveError, ContentError, ContextualCause, ContextualResult, DocumentError, ErrorCause,
    FilesystemError, WithContext, contextual,
};
use confit_core::sha::Sha;
use confit_ui::Event;
use confit_ui::ToUi;
use crossbeam_channel::Sender;

use crate::handles::{ArchiveHandle, ResourceHandle};
use crate::resources::Resources;
use confit_driver as driver;

/// The number of leading bytes that identify an archive format.
const HEAD_LEN: u64 = 512;

/// An index that maps each member name to its content hash and mode.
type Map = BTreeMap<String, (Sha, u32)>;

/// A file-backed store that lists archive members and extracts them into the cache.
#[derive(Debug, Clone)]
pub struct ArchiveStore {
    resources: Arc<Resources>,
    to_ui: Sender<ToUi>,
    maps: Arc<std::sync::RwLock<HashMap<Sha, Map>>>,
}

impl ArchiveStore {
    /// Builds a store over the shared resource cache.
    pub fn new(resources: Arc<Resources>, to_ui: Sender<ToUi>) -> Self {
        Self {
            resources,
            to_ui,
            maps: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Verifies one trusted source and returns its archive handle.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::ArchiveError`] when the source is not an archive.
    /// - [`confit_core::error::FilesystemError`] when the source is unreadable.
    pub fn archive(&self, source: &ResourceHandle) -> ContextualResult<ArchiveHandle> {
        match source {
            ResourceHandle::Project(path, sha) => {
                self.probe(path)?;
                Ok(ArchiveHandle::new(sha.clone(), path.clone()))
            }
            ResourceHandle::Cache(sha) => {
                let path = self.resources.cache_path(sha);
                self.resources.open(source)?;
                self.probe(&path)?;
                Ok(ArchiveHandle::new(sha.clone(), path))
            }
        }
    }

    /// Checks one path for a supported archive format.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::NotArchive`] when the source is not an archive.
    /// - [`FilesystemError`] when the source is unreadable.
    fn probe(&self, path: &Path) -> ContextualResult<()> {
        use std::io::Read as _;

        let file = std::fs::File::open(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)?;
        let mut head = Vec::new();
        std::io::Read::read_to_end(&mut file.take(HEAD_LEN), &mut head)
            .map_err(ContextualCause::from)
            .with_fs_path(path)?;
        if driver::archive::supported(&head) {
            return Ok(());
        }
        Err(contextual(ArchiveError::NotArchive).with_fs_path(path))
    }

    /// Returns the member names held by the archive index.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::ArchiveError`] when the archive is unreadable or corrupt.
    /// - [`confit_core::error::FilesystemError`] when the archive is unreadable or absent.
    pub fn members(&self, archive: &ArchiveHandle) -> ContextualResult<Vec<String>> {
        if let Some(map) = self.read_map(archive.sha256()) {
            return Ok(map.keys().cloned().collect());
        }
        Ok(self.unpack_all(archive)?.keys().cloned().collect())
    }

    /// Resolves one member to a trusted resource handle.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError`] when the archive is unreadable or corrupt.
    /// - [`FilesystemError`] when the archive is unreadable, absent, or the member is absent.
    pub fn member(&self, archive: &ArchiveHandle, name: &str) -> ContextualResult<ResourceHandle> {
        let archive_path = archive.path().to_path_buf();
        if let Some(map) = self.read_map(archive.sha256()) {
            match map.get(name) {
                Some((sha, _)) => {
                    let handle = ResourceHandle::Cache(sha.clone());
                    match self.resources.open(&handle) {
                        Ok(_) => return Ok(handle),
                        Err(error)
                            if matches!(
                                error.cause(),
                                ErrorCause::FilesystemError(FilesystemError::FileMissing)
                            ) => {}
                        Err(error) => return Err(error),
                    }
                }
                None => {
                    return Err(member_missing(&archive_path, name));
                }
            }
        }
        match self.unpack_all(archive) {
            Ok(map) => match map.get(name) {
                Some((sha, _)) => Ok(ResourceHandle::Cache(sha.clone())),
                None => Err(member_missing(&archive_path, name)),
            },
            Err(error)
                if matches!(
                    error.cause(),
                    ErrorCause::FilesystemError(FilesystemError::FileMissing)
                ) =>
            {
                Err(member_missing(&archive_path, name))
            }
            Err(error) => Err(error),
        }
    }

    /// Returns the archive index path under the cache.
    fn map_path(&self, sha: &Sha) -> PathBuf {
        self.resources.cache_path(sha).with_extension("map")
    }

    /// Loads the archive index from its cache file.
    fn read_map(&self, sha: &Sha) -> Option<Map> {
        if let Ok(guard) = self.maps.read()
            && let Some(map) = guard.get(sha)
        {
            return Some(map.clone());
        }
        let bytes = std::fs::read(self.map_path(sha)).ok()?;
        let map: Map = serde_json::from_slice(&bytes).ok()?;
        if let Ok(mut guard) = self.maps.write() {
            guard.insert(sha.clone(), map.clone());
        }
        Some(map)
    }

    /// Writes one archive index through a staged file.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::RenderFailed`] when the index fails to encode.
    /// - [`FilesystemError`] when the index fails to write.
    fn write_map(&self, archive: &ArchiveHandle, map: &Map) -> ContextualResult<()> {
        let archive_path = archive.path().to_path_buf();
        let map_path = self.map_path(archive.sha256());
        let bytes = serde_json::to_vec(map)
            .map_err(|_| contextual(DocumentError::RenderFailed).with_fs_path(&archive_path))?;
        driver::StagedFile::write(&map_path, |file| std::io::Write::write_all(file, &bytes))
            .with_cache_path(&map_path)?;
        if let Ok(mut guard) = self.maps.write() {
            guard.insert(archive.sha256().clone(), map.clone());
        }
        Ok(())
    }

    /// Extracts every member into the cache and writes a fresh index.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError`] when the archive is unreadable or corrupt.
    /// - [`FilesystemError`] when the archive is unreadable or a cache write fails.
    fn unpack_all(&self, archive: &ArchiveHandle) -> ContextualResult<Map> {
        let archive_path = archive.path().to_path_buf();
        let _ = self.to_ui.send(ToUi::Progress(Event::Started {
            id: archive_path.display().to_string(),
            label: format!("collecting artifacts: {}", archive_path.display()),
        }));
        let file = std::fs::File::open(&archive_path)
            .map_err(ContextualCause::from)
            .with_fs_path(&archive_path)?;
        let map = Unpack::new(&self.resources, &archive_path).run(file)?;
        self.write_map(archive, &map)?;
        let total = map.len();
        let _ = self.to_ui.send(ToUi::Progress(Event::Finished {
            id: archive_path.display().to_string(),
            label: format!(
                "collecting artifacts: {} ({total} members)",
                archive_path.display()
            ),
        }));
        Ok(map)
    }
}

/// The extraction state of one archive.
struct Unpack<'a> {
    /// The resources that store member bytes.
    resources: &'a Resources,
    /// The archive path for errors.
    archive_path: PathBuf,
    /// The extracted members keyed by name with their mode.
    entries: Map,
}

impl<'a> Unpack<'a> {
    /// Builds an empty walk for one archive path.
    fn new(resources: &'a Resources, archive_path: &Path) -> Self {
        Self {
            resources,
            archive_path: archive_path.to_path_buf(),
            entries: Map::new(),
        }
    }

    /// Extracts every archive entry into the landed index.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError`] when the archive is unreadable or corrupt.
    /// - [`FilesystemError`] when the archive is unreadable or a cache write fails.
    fn run(mut self, file: std::fs::File) -> ContextualResult<Map> {
        let archive_path = self.archive_path.clone();
        driver::archive::uncompress(file, |entry| self.entry(entry)).map_err(
            |error| match error.context.fs_path {
                Some(_) => error,
                None => error.with_fs_path(&archive_path),
            },
        )?;
        Ok(self.entries)
    }

    /// Stores one extracted entry in the cache.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] when a cache write fails.
    fn entry(&mut self, entry: driver::archive::Entry<'_>) -> ContextualResult<()> {
        let driver::archive::Entry { name, mode, reader } = entry;
        self.land(name, mode, reader)
    }

    /// Stores one member body in the cache under its content hash.
    ///
    /// # Errors
    ///
    /// - [`ContentError::ProjectHandleInCache`] when a project handle lands in the cache.
    /// - [`FilesystemError`] when a cache write fails.
    fn land(
        &mut self,
        name: String,
        mode: u32,
        reader: impl std::io::Read,
    ) -> ContextualResult<()> {
        let handle = self.resources.store(reader)?;
        match handle {
            ResourceHandle::Cache(sha) => {
                self.entries.insert(name, (sha, mode));
                Ok(())
            }
            ResourceHandle::Project(..) => Err(contextual(ContentError::ProjectHandleInCache)),
        }
    }
}

/// Creates the error for an absent member with its archive path.
fn member_missing(archive_path: &Path, name: &str) -> ContextualCause {
    contextual(FilesystemError::FileMissing)
        .with_fs_path(archive_path)
        .with_member(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreRoots;
    use crate::pool::PoolStore;
    use confit_core::error::ErrorCause;
    use confit_driver::archive::{Algorithm, Builder, Level};
    use std::io::Read as _;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            temp_base: dir.join("temp"),
            ..Default::default()
        }
    }

    fn test_store(dir: &Path) -> ArchiveStore {
        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let pools = std::sync::Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = std::sync::Arc::new(Resources::new(&roots, pools));
        ArchiveStore::new(resources, sender)
    }

    fn test_resources(dir: &Path) -> Resources {
        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let pools = std::sync::Arc::new(PoolStore::new(&roots, sender));
        Resources::new(&roots, pools)
    }

    fn tar_gz_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::TarGz, Level::High);
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

    fn write_archive(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn project_source(dir: &Path, path: PathBuf) -> ResourceHandle {
        assert!(path.starts_with(dir), "the project is under its root");
        let sha = std::fs::read(&path)
            .map(|bytes| Sha::hash(&bytes))
            .unwrap_or_else(|_| Sha::hash(b"absent"));
        ResourceHandle::Project(path, sha)
    }

    fn fixture_archive(
        store: &ArchiveStore,
        dir: &Path,
        name: &str,
        bytes: &[u8],
    ) -> ArchiveHandle {
        let path = write_archive(dir, name, bytes);
        let source = project_source(dir, path);
        match store.archive(&source) {
            Ok(handle) => handle,
            Err(error) => panic!("the source fails to register: {error}"),
        }
    }

    fn cache_path(dir: &Path, sha: &Sha) -> PathBuf {
        dir.join("temp").join("cache").join(sha.hex())
    }

    fn map_path(dir: &Path, archive: &ArchiveHandle) -> PathBuf {
        cache_path(dir, archive.sha256()).with_extension("map")
    }

    fn open_text(resources: &Resources, handle: &ResourceHandle) -> Vec<u8> {
        match resources.open(handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                found
            }
            Err(error) => panic!("the member fails to open: {error}"),
        }
    }

    #[test]
    fn create_registers_the_project_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(&[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.tar.gz", &raw);
        let source = project_source(dir.path(), path.clone());
        match store.archive(&source) {
            Ok(handle) => {
                assert_eq!(handle.sha256(), &Sha::hash(&raw));
                assert_eq!(
                    handle.path(),
                    path.as_path(),
                    "the handle keeps the project path"
                );
                assert!(
                    !cache_path(dir.path(), handle.sha256()).exists(),
                    "the project bytes match"
                );
            }
            Err(error) => panic!("the source fails to register: {error}"),
        }
    }

    #[test]
    fn create_rejects_plain_and_absent_sources() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let path = write_archive(dir.path(), "note.txt", b"plain text");
        let source = project_source(dir.path(), path.clone());
        match store.archive(&source) {
            Ok(_) => panic!("expected a text source"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::ArchiveError(ArchiveError::NotArchive)
                    ),
                    "the text source keeps its not-archive cause: {error}"
                );
                match &error.context.fs_path {
                    Some(found) => {
                        assert_eq!(found, &path, "the text source keeps the archive path")
                    }
                    None => panic!("expected the archive path: {error}"),
                };
            }
        }
        let missing = dir.path().join("absent.tar.gz");
        let absent = project_source(dir.path(), missing.clone());
        match store.archive(&absent) {
            Ok(_) => panic!("expected an absent source"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent source reports a missing file: {error}"
                );
            }
        }
    }

    #[test]
    fn members_lists_files_and_skips_folders() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("the members fail to list: {error}"),
        }
    }

    #[test]
    fn members_serve_from_the_index_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.members(&archive) {
            Ok(_) => {}
            Err(error) => panic!("the index fails to build: {error}"),
        }
        std::fs::remove_file(dir.path().join("fonts.tar.gz")).unwrap();
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("the present index fails to list: {error}"),
        }
    }

    #[test]
    fn member_resolves_from_the_cache_only() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.member(&archive, "a.txt") {
            Ok(ResourceHandle::Cache(sha)) => assert_eq!(sha, Sha::hash(b"alpha")),
            Ok(ResourceHandle::Project(..)) => panic!("expected a cache handle"),
            Err(error) => panic!("the member fails to resolve: {error}"),
        }
        std::fs::remove_file(dir.path().join("fonts.tar.gz")).unwrap();
        match store.member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"alpha"),
            Err(error) => panic!("the member fails to resolve from the index: {error}"),
        }
    }

    #[test]
    fn member_rewrites_an_absent_index() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.member(&archive, "a.txt") {
            Ok(_) => {}
            Err(error) => panic!("the member fails to resolve: {error}"),
        }
        std::fs::remove_file(map_path(dir.path(), &archive)).unwrap();
        let store = test_store(dir.path());
        match store.member(&archive, "sub/b.txt") {
            Ok(ResourceHandle::Cache(sha)) => assert_eq!(sha, Sha::hash(b"beta")),
            Ok(ResourceHandle::Project(..)) => panic!("expected a cache handle"),
            Err(error) => panic!("the absent index fails to rebuild: {error}"),
        }
        assert!(
            map_path(dir.path(), &archive).exists(),
            "the rebuild writes the index"
        );
    }

    #[test]
    fn member_repacks_absent_member_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.member(&archive, "a.txt") {
            Ok(_) => {}
            Err(error) => panic!("the member fails to resolve: {error}"),
        }
        std::fs::remove_file(cache_path(dir.path(), &Sha::hash(b"beta"))).unwrap();
        match store.member(&archive, "sub/b.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"beta"),
            Err(error) => panic!("the absent member bytes fail to repack: {error}"),
        }
    }

    #[test]
    fn member_rejects_an_absent_archive_and_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha")]),
        );
        match store.members(&archive) {
            Ok(_) => {}
            Err(error) => panic!("the index fails to build: {error}"),
        }
        std::fs::remove_file(map_path(dir.path(), &archive)).unwrap();
        std::fs::remove_file(dir.path().join("fonts.tar.gz")).unwrap();
        let store = test_store(dir.path());
        match store.member(&archive, "a.txt") {
            Ok(_) => panic!("expected an absent archive"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent archive reports a missing file: {error}"
                );
                match &error.context.fs_path {
                    Some(found) => assert_eq!(
                        found,
                        &dir.path().join("fonts.tar.gz"),
                        "the absent archive keeps the archive path"
                    ),
                    None => panic!("expected the archive path: {error}"),
                }
                match &error.context.member {
                    Some(member) => {
                        assert_eq!(member, "a.txt", "the absent archive keeps the member")
                    }
                    None => panic!("expected the member name: {error}"),
                }
            }
        }
        match store.member(&archive, "absent.txt") {
            Ok(_) => panic!("expected an absent member"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent member reports a missing file: {error}"
                );
                match &error.context.fs_path {
                    Some(found) => assert_eq!(
                        found,
                        &dir.path().join("fonts.tar.gz"),
                        "the absent member keeps the archive path"
                    ),
                    None => panic!("expected the archive path: {error}"),
                }
                match &error.context.member {
                    Some(member) => {
                        assert_eq!(member, "absent.txt", "the absent member keeps the member")
                    }
                    None => panic!("expected the member name: {error}"),
                }
            }
        }
    }

    #[test]
    fn unpack_records_index_with_hashes_and_modes() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.member(&archive, "sub/b.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"beta"),
            Err(error) => panic!("the member fails to resolve: {error}"),
        }
        let text = match std::fs::read_to_string(map_path(dir.path(), &archive)) {
            Ok(text) => text,
            Err(error) => panic!("the index fails to load: {error}"),
        };
        assert!(
            text.contains(&Sha::hash(b"beta").hex()),
            "the index holds every hash: {text}"
        );
        assert!(
            text.contains("sub/b.txt"),
            "the index holds every name: {text}"
        );
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(map)) => match map.get("a.txt") {
                Some(serde_json::Value::Array(pair)) => {
                    assert_eq!(pair.len(), 2, "the index pairs each hash with its mode");
                    assert_eq!(
                        pair[0],
                        serde_json::Value::String(Sha::hash(b"alpha").hex())
                    );
                    assert_eq!(pair[1], serde_json::Value::Number(0o644.into()));
                }
                other => panic!("expected a hash and mode pair: {other:?}"),
            },
            other => panic!("expected an object index: {other:?}"),
        }
        assert!(
            !dir.path().join("temp").join("extract").exists(),
            "the unpack leaves no extract folder"
        );
    }

    #[test]
    fn bare_gzip_rejects_listing_and_lookup() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        use std::io::Write as _;
        encoder.write_all(b"plain").unwrap();
        let raw = encoder.finish().unwrap();
        let landed = cache_path(dir.path(), &Sha::hash(&raw));
        std::fs::create_dir_all(landed.parent().unwrap()).unwrap();
        std::fs::write(&landed, &raw).unwrap();
        let archive = ArchiveHandle::new(Sha::hash(&raw), landed);
        match store.members(&archive) {
            Ok(_) => panic!("expected a bare gzip error"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::ArchiveError(ArchiveError::BareGzip)
                    ),
                    "the bare gzip keeps its not-archive cause: {error}"
                );
            }
        }
        match store.member(&archive, "note.txt") {
            Ok(_) => panic!("expected a bare gzip error"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::ArchiveError(ArchiveError::BareGzip)
                    ),
                    "the bare gzip keeps its not-archive cause: {error}"
                );
            }
        }
    }

    #[test]
    fn members_rejects_an_absent_archive() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let handle = ArchiveHandle::new(Sha::hash(b"absent"), dir.path().join("absent.cb"));
        match store.members(&handle) {
            Ok(_) => panic!("expected an absent archive"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent archive reports a missing file: {error}"
                );
            }
        }
    }

    #[test]
    fn member_keeps_one_cache_entry_per_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let raw_a = tar_gz_bytes(&[("a.txt", b"alpha")]);
        let raw_b = tar_gz_bytes(&[("b.txt", b"alpha")]);
        let sha_a = Sha::hash(&raw_a);
        let sha_b = Sha::hash(&raw_b);
        assert_ne!(sha_a, sha_b, "the fixtures differ by member name");
        let first = fixture_archive(&store, dir.path(), "a.tar.gz", &raw_a);
        let second = fixture_archive(&store, dir.path(), "b.tar.gz", &raw_b);
        assert_eq!(first.sha256(), &sha_a);
        assert_eq!(second.sha256(), &sha_b);
        let first_handle = match store.member(&first, "a.txt") {
            Ok(handle) => handle,
            Err(error) => panic!("the first member fails to resolve: {error}"),
        };
        let second_handle = match store.member(&second, "b.txt") {
            Ok(handle) => handle,
            Err(error) => panic!("the second member fails to resolve: {error}"),
        };
        assert_eq!(first_handle, second_handle);
        assert_ne!(map_path(dir.path(), &first), map_path(dir.path(), &second));
        assert_eq!(open_text(&resources, &first_handle), b"alpha");
    }

    #[test]
    fn member_streams_large_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let raw: Vec<u8> = (0..20 * 1024).map(|index| (index % 251) as u8).collect();
        assert!(raw.len() > 8 * 1024, "the payload spans many chunks");
        let archive = fixture_archive(
            &store,
            dir.path(),
            "big.tar.gz",
            &tar_gz_bytes(&[("big.bin", &raw)]),
        );
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["big.bin"]),
            Err(error) => panic!("the large members fail to list: {error}"),
        }
        match store.member(&archive, "big.bin") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), raw),
            Err(error) => panic!("the large member fails to resolve: {error}"),
        }
    }

    fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
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

    fn zip_bytes_with_dir() -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        let mut empty = std::io::Cursor::new(b"" as &[u8]);
        if let Err(error) = builder.member("sub/", 0o755, 0, &mut empty) {
            panic!("the fixture fails to build: {error}");
        }
        for (name, bytes) in [
            ("a.txt", b"alpha" as &[u8]),
            ("sub/b.txt", b"beta" as &[u8]),
        ] {
            let mut reader = std::io::Cursor::new(bytes);
            if let Err(error) = builder.member(name, 0o644, bytes.len() as u64, &mut reader) {
                panic!("the fixture fails to build: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("the fixture fails to finish: {error}"),
        }
    }

    fn symlink_zip_bytes() -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        let mut reader = std::io::Cursor::new(b"a.txt" as &[u8]);
        if let Err(error) = builder.member("link.txt", 0o120777, 5, &mut reader) {
            panic!("the fixture fails to build: {error}");
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("the fixture fails to finish: {error}"),
        }
    }

    #[test]
    fn zip_create_registers_the_project_and_rejects_text() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw = zip_bytes(&[("a.txt", b"alpha")]);
        let path = write_archive(dir.path(), "fonts.zip", &raw);
        let source = project_source(dir.path(), path.clone());
        match store.archive(&source) {
            Ok(handle) => {
                assert_eq!(handle.sha256(), &Sha::hash(&raw));
                assert_eq!(
                    handle.path(),
                    path.as_path(),
                    "the handle keeps the project path"
                );
                assert!(
                    !cache_path(dir.path(), handle.sha256()).exists(),
                    "the zip bytes match"
                );
            }
            Err(error) => panic!("the zip source fails to register: {error}"),
        }
        let plain = write_archive(dir.path(), "note.txt", b"plain text");
        let plain_source = project_source(dir.path(), plain.clone());
        match store.archive(&plain_source) {
            Ok(_) => panic!("expected a text source"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::ArchiveError(ArchiveError::NotArchive)
                    ),
                    "the text source keeps its not-archive cause: {error}"
                );
            }
        }
    }

    #[test]
    fn zip_members_list_files_and_skip_folders() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(&store, dir.path(), "fonts.zip", &zip_bytes_with_dir());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("the zip members fail to list: {error}"),
        }
    }

    #[test]
    fn zip_member_unpacks_and_reuses_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.zip",
            &zip_bytes(&[("a.txt", b"alpha"), ("sub/b.txt", b"beta")]),
        );
        match store.member(&archive, "sub/b.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"beta"),
            Err(error) => panic!("the zip member fails to resolve: {error}"),
        }
        match store.member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"alpha"),
            Err(error) => panic!("the repeated member fails to reuse the cache: {error}"),
        }
        assert!(
            !dir.path().join("temp").join("extract").exists(),
            "the zip unpack leaves no extract folder"
        );
    }

    #[test]
    fn zip_symlink_mode_extracts_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(&store, dir.path(), "link.zip", &symlink_zip_bytes());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["link.txt"]),
            Err(error) => panic!("the symlink members fail to list: {error}"),
        }
        match store.member(&archive, "link.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"a.txt"),
            Err(error) => panic!("the symlink member fails to resolve: {error}"),
        }
    }

    #[test]
    fn unpack_announces_the_member_count() {
        use confit_ui::Event;

        let dir = tempfile::tempdir().unwrap();
        let roots = test_roots(dir.path());
        let (sender, receiver) = crossbeam_channel::unbounded();
        let pools = std::sync::Arc::new(PoolStore::new(&roots, sender.clone()));
        let resources = std::sync::Arc::new(Resources::new(&roots, pools));
        let store = ArchiveStore::new(resources, sender);
        let archive = fixture_archive(
            &store,
            dir.path(),
            "fonts.tar.gz",
            &tar_gz_bytes(&[("a.txt", b"alpha"), ("b.txt", b"beta")]),
        );
        match store.members(&archive) {
            Ok(names) => {
                assert_eq!(names.len(), 2, "the unpack lists two members")
            }
            Err(error) => panic!("the archive fails to unpack: {error}"),
        }
        let events: Vec<ToUi> = receiver.try_iter().collect();
        assert_eq!(events.len(), 2, "the unpack writes two events");
        match &events[1] {
            ToUi::Progress(Event::Finished { id: seen, label }) => {
                assert!(
                    label.contains("(2 members)"),
                    "the unpack counts every member: {label}"
                );
                assert_eq!(
                    seen,
                    &dir.path()
                        .join("fonts.tar.gz")
                        .to_string_lossy()
                        .into_owned(),
                    "the unpack reports the archive path: {seen}"
                );
            }
            other => panic!("expected a finished event: {other:?}"),
        }
    }
}
