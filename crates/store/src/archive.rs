//! Archive
//! Member listing and extraction for compressed archives.

pub mod error;

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_driver::progress::{Event, ProgressSender};
use confit_driver::sha::Sha;

use crate::faults::AccessFault;
use crate::handles::{ArchiveHandle, ResourceHandle};
use crate::resources::Resources;
use crate::resources::error::ResourceError;
use confit_driver as driver;
use error::{ArchiveError, Result};

/// Head window for every probe, wide enough for tar magic.
const HEAD_LEN: u64 = 512;

/// Archive index of member names to content hashes with modes.
type Map = BTreeMap<String, (Sha, u32)>;

/// File-backed archive member listing and extraction.
///
/// Members are archive-relative with forward slashes.
#[derive(Debug, Clone)]
pub struct ArchiveStore {
    resources: Arc<Resources>,
    progress: ProgressSender,
    maps: Arc<std::sync::RwLock<HashMap<Sha, Map>>>,
}

impl ArchiveStore {
    /// Build an archive store over the shared cache.
    pub fn new(resources: Arc<Resources>, progress: ProgressSender) -> Self {
        Self {
            resources,
            progress,
            maps: Arc::new(std::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Register one trusted source as a verified archive.
    ///
    /// - Project sources keep their bytes in place
    /// - Cache entries trust the landed bytes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable sources.
    /// - [`ArchiveError::NotArchive`] for non-archives.
    pub fn archive(&self, source: &ResourceHandle) -> Result<ArchiveHandle> {
        match source {
            ResourceHandle::Project(path, sha) => {
                self.probe(path)?;
                Ok(ArchiveHandle::new(sha.clone(), path.clone()))
            }
            ResourceHandle::Cache(sha) => {
                let path = self.resources.cache_path(sha);
                self.resources
                    .open(source)
                    .map_err(|error| from_resource(&path, error))?;
                self.probe(&path)?;
                Ok(ArchiveHandle::new(sha.clone(), path))
            }
        }
    }

    /// Reject non-archive bytes at one path.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable sources.
    /// - [`ArchiveError::NotArchive`] for non-archives.
    fn probe(&self, path: &Path) -> Result<()> {
        use std::io::Read as _;

        let file = std::fs::File::open(path).map_err(|error| ArchiveError::from_io(path, error))?;
        let mut head = Vec::new();
        std::io::Read::read_to_end(&mut file.take(HEAD_LEN), &mut head)
            .map_err(|error| ArchiveError::from_io(path, error))?;
        if driver::archive::supported(&head) {
            return Ok(());
        }
        Err(ArchiveError::NotArchive {
            path: path.to_path_buf(),
        })
    }

    /// List member names from the archive index.
    ///
    /// - Keys come straight from the map with no archive IO.
    /// - A missing index unpacks from the archive bytes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable archives.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    pub fn members(&self, archive: &ArchiveHandle) -> Result<Vec<String>> {
        if let Some(map) = self.read_map(archive.sha256()) {
            return Ok(map.keys().cloned().collect());
        }
        Ok(self.unpack_all(archive)?.keys().cloned().collect())
    }

    /// Pick one archive member by its archive-relative name.
    ///
    /// - Names are archive-relative with forward slashes.
    /// - Present entries trust the cache with no re-hash.
    /// - A missing index or missing member bytes unpacks from the
    ///   archive bytes.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Escape`] for escaping members.
    /// - [`ArchiveError::UnknownMember`] for unknown members and for
    ///   members of absent archive bytes.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    pub fn member(&self, archive: &ArchiveHandle, name: &str) -> Result<ResourceHandle> {
        let archive_path = archive.path().to_path_buf();
        if let Some(map) = self.read_map(archive.sha256()) {
            match map.get(name) {
                Some((sha, _)) => {
                    let handle = ResourceHandle::Cache(sha.clone());
                    match self.resources.open(&handle) {
                        Ok(_) => return Ok(handle),
                        Err(ResourceError::Read {
                            fault: AccessFault::Missing,
                            ..
                        }) => {}
                        Err(error) => {
                            return Err(from_resource(&self.resources.cache_path(sha), error));
                        }
                    }
                }
                None => {
                    return Err(ArchiveError::UnknownMember {
                        path: archive_path,
                        name: name.to_owned(),
                    });
                }
            }
        }
        match self.unpack_all(archive) {
            Ok(map) => match map.get(name) {
                Some((sha, _)) => Ok(ResourceHandle::Cache(sha.clone())),
                None => Err(ArchiveError::UnknownMember {
                    path: archive_path,
                    name: name.to_owned(),
                }),
            },
            Err(ArchiveError::Read {
                fault: AccessFault::Missing,
                ..
            }) => Err(ArchiveError::UnknownMember {
                path: archive_path,
                name: name.to_owned(),
            }),
            Err(error) => Err(error),
        }
    }

    /// Derive the index path for one archive sha.
    fn map_path(&self, sha: &Sha) -> PathBuf {
        self.resources.cache_path(sha).with_extension("map")
    }

    /// Load one archive index. Corrupt payloads read absent.
    ///
    /// - Poisoned locks read through to disk.
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

    /// Rewrite one archive index atomically.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Write`] for index write faults.
    /// - [`ArchiveError::WriteUnknown`] for other index write failures.
    fn write_map(&self, archive: &ArchiveHandle, map: &Map) -> Result<()> {
        let archive_path = archive.path().to_path_buf();
        let bytes = serde_json::to_vec(map).map_err(|error| ArchiveError::WriteUnknown {
            path: archive_path.clone(),
            message: error.to_string(),
        })?;
        driver::StagedFile::write(&self.map_path(archive.sha256()), |file| {
            std::io::Write::write_all(file, &bytes)
        })
        .map_err(|error| ArchiveError::from_write_io(&archive_path, error))?;
        if let Ok(mut guard) = self.maps.write() {
            guard.insert(archive.sha256().clone(), map.clone());
        }
        Ok(())
    }

    /// Unpack every member into the cache with a fresh index.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable archives.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::Escape`] for escaping members.
    /// - [`ArchiveError::Write`] for cache write faults.
    /// - [`ArchiveError::WriteUnknown`] for other cache write failures.
    fn unpack_all(&self, archive: &ArchiveHandle) -> Result<Map> {
        let archive_path = archive.path().to_path_buf();
        let file = std::fs::File::open(&archive_path)
            .map_err(|error| ArchiveError::from_io(&archive_path, error))?;
        let map = Unpack::new(&self.resources, &archive_path).run(file)?;
        self.write_map(archive, &map)?;
        let total = map.len();
        let _ = self.progress.send(Event::Unpacked {
            archive: archive_path.display().to_string(),
            total,
        });
        Ok(map)
    }
}

/// One archive walk landing members in the cache.
struct Unpack<'a> {
    /// Resources landing member bytes.
    resources: &'a Resources,
    /// Archive cache path for errors.
    archive_path: PathBuf,
    /// Landed members by name with modes.
    entries: Map,
}

impl<'a> Unpack<'a> {
    /// Build one archive walk.
    fn new(resources: &'a Resources, archive_path: &Path) -> Self {
        Self {
            resources,
            archive_path: archive_path.to_path_buf(),
            entries: Map::new(),
        }
    }

    /// Run one walk into the landed index.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable archives.
    /// - [`ArchiveError::CorruptedArchive`] for corrupt archives.
    /// - [`ArchiveError::Escape`] for escaping members.
    /// - [`ArchiveError::Write`] for cache write faults.
    /// - [`ArchiveError::WriteUnknown`] for other cache write failures.
    fn run(mut self, file: std::fs::File) -> Result<Map> {
        let archive_path = self.archive_path.clone();
        driver::archive::uncompress(file, |entry| self.entry(entry))
            .map_err(|error| Self::driver_error(&archive_path, error))?;
        Ok(self.entries)
    }

    /// Receive one walk entry into the cache.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Escape`] for escaping members.
    /// - [`ArchiveError::CorruptedArchive`] for members failing the
    ///   expected hash.
    /// - [`ArchiveError::Write`] for cache write faults.
    /// - [`ArchiveError::WriteUnknown`] for other cache write failures.
    fn entry(&mut self, entry: driver::archive::Entry<'_>) -> Result<()> {
        let driver::archive::Entry { name, mode, reader } = entry;
        self.land(name, mode, reader)
    }

    /// Land one member body in the cache under its hash.
    ///
    /// # Errors
    ///
    /// - [`ArchiveError::Read`] for unreadable members.
    /// - [`ArchiveError::CorruptedArchive`] for members failing the
    ///   expected hash.
    /// - [`ArchiveError::Write`] for cache write faults.
    /// - [`ArchiveError::WriteUnknown`] for other cache write failures.
    fn land(&mut self, name: String, mode: u32, reader: impl std::io::Read) -> Result<()> {
        let handle = self
            .resources
            .store(reader)
            .map_err(|error| from_resource(&self.archive_path, error))?;
        match handle {
            ResourceHandle::Cache(sha) => {
                self.entries.insert(name, (sha, mode));
                Ok(())
            }
            ResourceHandle::Project(..) => Err(ArchiveError::CorruptedArchive {
                path: self.archive_path.clone(),
            }),
        }
    }

    /// Map one driver archive failure into domain language.
    fn driver_error(source: &Path, error: driver::archive::Error<ArchiveError>) -> ArchiveError {
        match error {
            driver::archive::Error::Visit(inner) => inner,
            driver::archive::Error::Foreign => ArchiveError::NotArchive {
                path: source.to_path_buf(),
            },
            driver::archive::Error::Bare
            | driver::archive::Error::Corrupt(_)
            | driver::archive::Error::Truncated(_) => ArchiveError::CorruptedArchive {
                path: source.to_path_buf(),
            },
            driver::archive::Error::Locked => ArchiveError::PasswordProtectedArchive {
                path: source.to_path_buf(),
            },
            driver::archive::Error::Unknown { detail } => ArchiveError::Unknown {
                path: source.to_path_buf(),
                detail,
            },
            driver::archive::Error::UnknownAlgorithm { detail } => ArchiveError::UnknownAlgorithm {
                path: source.to_path_buf(),
                detail,
            },
            driver::archive::Error::Denied(inner)
            | driver::archive::Error::Missing(inner)
            | driver::archive::Error::Invalid(inner)
            | driver::archive::Error::Io(inner) => from_stream(source, inner),
        }
    }
}

/// Map one stream failure into its archive error.
fn from_stream(source: &Path, error: std::io::Error) -> ArchiveError {
    match error.kind() {
        std::io::ErrorKind::StorageFull
        | std::io::ErrorKind::ReadOnlyFilesystem
        | std::io::ErrorKind::QuotaExceeded
        | std::io::ErrorKind::FileTooLarge => ArchiveError::from_write_io(source, error),
        std::io::ErrorKind::Other
        | std::io::ErrorKind::InvalidInput
        | std::io::ErrorKind::InvalidData
        | std::io::ErrorKind::UnexpectedEof => ArchiveError::CorruptedArchive {
            path: source.to_path_buf(),
        },
        _ => ArchiveError::from_io(source, error),
    }
}

/// Map one resource creation failure into archive language.
///
/// - Hashed members failing the expected hash mean a corrupt archive.
fn from_resource(source: &Path, error: ResourceError) -> ArchiveError {
    match error {
        ResourceError::Escape { path, .. } => ArchiveError::Escape { path },
        ResourceError::Read { path, fault } => ArchiveError::Read { path, fault },
        ResourceError::Write { path, fault } => ArchiveError::Write { path, fault },
        ResourceError::WriteUnknown { path, message } => {
            ArchiveError::WriteUnknown { path, message }
        }
        ResourceError::Mismatch { .. } => ArchiveError::CorruptedArchive {
            path: source.to_path_buf(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreRoots;
    use crate::pool::PoolStore;
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
                panic!("fixture builds: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("fixture finishes: {error}"),
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
        assert!(path.starts_with(dir), "project stays under its root");
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
            Err(error) => panic!("source registers: {error}"),
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
            Err(error) => panic!("member opens: {error}"),
        }
    }

    #[test]
    fn create_registers_project_without_copy() {
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
                    "handle keeps the project path"
                );
                assert!(
                    !cache_path(dir.path(), handle.sha256()).exists(),
                    "project bytes stay in place"
                );
            }
            Err(error) => panic!("source registers: {error}"),
        }
    }

    #[test]
    fn create_rejects_plain_and_missing_sources() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let path = write_archive(dir.path(), "note.txt", b"plain text");
        let source = project_source(dir.path(), path.clone());
        match store.archive(&source) {
            Ok(_) => panic!("plain source passes"),
            Err(ArchiveError::NotArchive { .. }) => {}
            Err(error) => panic!("wrong plain variant: {error}"),
        }
        let missing = dir.path().join("absent.tar.gz");
        let absent = project_source(dir.path(), missing.clone());
        match store.archive(&absent) {
            Ok(_) => panic!("absent source passes"),
            Err(ArchiveError::Read { .. }) => {}
            Err(error) => panic!("wrong absent variant: {error}"),
        }
    }

    #[test]
    fn members_lists_files_skipping_folders() {
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
            Err(error) => panic!("members list: {error}"),
        }
    }

    #[test]
    fn members_serve_from_index_without_archive_bytes() {
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
            Err(error) => panic!("index builds: {error}"),
        }
        std::fs::remove_file(dir.path().join("fonts.tar.gz")).unwrap();
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("present index skips archive bytes: {error}"),
        }
    }

    #[test]
    fn member_resolves_from_cache_without_archive_bytes() {
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
            Ok(ResourceHandle::Project(..)) => panic!("member lands content-addressed"),
            Err(error) => panic!("member resolves: {error}"),
        }
        std::fs::remove_file(dir.path().join("fonts.tar.gz")).unwrap();
        match store.member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"alpha"),
            Err(error) => panic!("present member skips archive bytes: {error}"),
        }
    }

    #[test]
    fn member_rewrites_missing_index() {
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
            Err(error) => panic!("member resolves: {error}"),
        }
        std::fs::remove_file(map_path(dir.path(), &archive)).unwrap();
        let store = test_store(dir.path());
        match store.member(&archive, "sub/b.txt") {
            Ok(ResourceHandle::Cache(sha)) => assert_eq!(sha, Sha::hash(b"beta")),
            Ok(ResourceHandle::Project(..)) => panic!("member lands content-addressed"),
            Err(error) => panic!("missing index repacks: {error}"),
        }
        assert!(
            map_path(dir.path(), &archive).exists(),
            "repack rewrites the index"
        );
    }

    #[test]
    fn member_repacks_missing_member_bytes() {
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
            Err(error) => panic!("member resolves: {error}"),
        }
        std::fs::remove_file(cache_path(dir.path(), &Sha::hash(b"beta"))).unwrap();
        match store.member(&archive, "sub/b.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"beta"),
            Err(error) => panic!("missing member bytes repack: {error}"),
        }
    }

    #[test]
    fn member_rejects_absent_without_archive_bytes() {
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
            Err(error) => panic!("index builds: {error}"),
        }
        std::fs::remove_file(map_path(dir.path(), &archive)).unwrap();
        std::fs::remove_file(dir.path().join("fonts.tar.gz")).unwrap();
        let store = test_store(dir.path());
        match store.member(&archive, "a.txt") {
            Ok(_) => panic!("absent archive passes"),
            Err(ArchiveError::UnknownMember { path, name }) => {
                assert_eq!(name, "a.txt");
                assert_eq!(path, dir.path().join("fonts.tar.gz"));
            }
            Err(error) => panic!("wrong absent variant: {error}"),
        }
        match store.member(&archive, "absent.txt") {
            Ok(_) => panic!("absent member passes"),
            Err(ArchiveError::UnknownMember { name, .. }) => assert_eq!(name, "absent.txt"),
            Err(error) => panic!("wrong absent variant: {error}"),
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
            Err(error) => panic!("member resolves: {error}"),
        }
        let text = match std::fs::read_to_string(map_path(dir.path(), &archive)) {
            Ok(text) => text,
            Err(error) => panic!("index load: {error}"),
        };
        assert!(
            text.contains(&Sha::hash(b"beta").hex()),
            "index records hashes: {text}"
        );
        assert!(text.contains("sub/b.txt"), "index records names: {text}");
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(serde_json::Value::Object(map)) => match map.get("a.txt") {
                Some(serde_json::Value::Array(pair)) => {
                    assert_eq!(pair.len(), 2, "index pairs hashes with modes");
                    assert_eq!(
                        pair[0],
                        serde_json::Value::String(Sha::hash(b"alpha").hex())
                    );
                    assert_eq!(pair[1], serde_json::Value::Number(0o644.into()));
                }
                other => panic!("index shapes pairs: {other:?}"),
            },
            other => panic!("index holds an object: {other:?}"),
        }
        assert!(
            !dir.path().join("temp").join("extract").exists(),
            "unpack leaves no extract folder"
        );
    }

    #[test]
    fn gzip_lists_and_opens_derived_member() {
        use std::io::Write as _;

        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(b"plain").unwrap();
        let raw = encoder.finish().unwrap();
        let landed = cache_path(dir.path(), &Sha::hash(&raw));
        std::fs::create_dir_all(landed.parent().unwrap()).unwrap();
        std::fs::write(&landed, &raw).unwrap();
        let archive = ArchiveHandle::new(Sha::hash(&raw), landed);
        match store.members(&archive) {
            Ok(_) => panic!("bare gzip passes"),
            Err(ArchiveError::CorruptedArchive { .. }) => {}
            Err(error) => panic!("wrong bare variant: {error}"),
        }
        match store.member(&archive, "note.txt") {
            Ok(_) => panic!("bare gzip passes"),
            Err(ArchiveError::CorruptedArchive { .. }) => {}
            Err(error) => panic!("wrong bare variant: {error}"),
        }
    }

    #[test]
    fn members_rejects_missing_archive() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let handle = ArchiveHandle::new(Sha::hash(b"absent"), dir.path().join("absent.cb"));
        match store.members(&handle) {
            Ok(_) => panic!("absent archive passes"),
            Err(ArchiveError::Read { .. }) => {}
            Err(error) => panic!("wrong absent variant: {error}"),
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
        assert_ne!(sha_a, sha_b, "fixtures diverge by member name");
        let first = fixture_archive(&store, dir.path(), "a.tar.gz", &raw_a);
        let second = fixture_archive(&store, dir.path(), "b.tar.gz", &raw_b);
        assert_eq!(first.sha256(), &sha_a);
        assert_eq!(second.sha256(), &sha_b);
        let first_handle = match store.member(&first, "a.txt") {
            Ok(handle) => handle,
            Err(error) => panic!("first member resolves: {error}"),
        };
        let second_handle = match store.member(&second, "b.txt") {
            Ok(handle) => handle,
            Err(error) => panic!("second member resolves: {error}"),
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
        assert!(raw.len() > 8 * 1024, "payload spans many chunks");
        let archive = fixture_archive(
            &store,
            dir.path(),
            "big.tar.gz",
            &tar_gz_bytes(&[("big.bin", &raw)]),
        );
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["big.bin"]),
            Err(error) => panic!("large members list: {error}"),
        }
        match store.member(&archive, "big.bin") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), raw),
            Err(error) => panic!("large member resolves: {error}"),
        }
    }

    fn zip_bytes(members: &[(&str, &[u8])]) -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        for (name, bytes) in members {
            let mut reader = std::io::Cursor::new(*bytes);
            if let Err(error) = builder.member(name, 0o644, bytes.len() as u64, &mut reader) {
                panic!("fixture builds: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("fixture finishes: {error}"),
        }
    }

    fn zip_bytes_with_dir() -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        let mut empty = std::io::Cursor::new(b"" as &[u8]);
        if let Err(error) = builder.member("sub/", 0o755, 0, &mut empty) {
            panic!("fixture builds: {error}");
        }
        for (name, bytes) in [
            ("a.txt", b"alpha" as &[u8]),
            ("sub/b.txt", b"beta" as &[u8]),
        ] {
            let mut reader = std::io::Cursor::new(bytes);
            if let Err(error) = builder.member(name, 0o644, bytes.len() as u64, &mut reader) {
                panic!("fixture builds: {error}");
            }
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("fixture finishes: {error}"),
        }
    }

    fn symlink_zip_bytes() -> Vec<u8> {
        let sink = std::io::Cursor::new(Vec::new());
        let mut builder = Builder::create(sink, Algorithm::Zip, Level::None);
        let mut reader = std::io::Cursor::new(b"a.txt" as &[u8]);
        if let Err(error) = builder.member("link.txt", 0o120777, 5, &mut reader) {
            panic!("fixture builds: {error}");
        }
        match builder.finish() {
            Ok(sink) => sink.into_inner(),
            Err(error) => panic!("fixture finishes: {error}"),
        }
    }

    #[test]
    fn zip_create_registers_and_rejects_plain() {
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
                    "handle keeps the project path"
                );
                assert!(
                    !cache_path(dir.path(), handle.sha256()).exists(),
                    "zip bytes stay in place"
                );
            }
            Err(error) => panic!("zip source registers: {error}"),
        }
        let plain = write_archive(dir.path(), "note.txt", b"plain text");
        let plain_source = project_source(dir.path(), plain.clone());
        match store.archive(&plain_source) {
            Ok(_) => panic!("plain source passes"),
            Err(ArchiveError::NotArchive { .. }) => {}
            Err(error) => panic!("wrong plain variant: {error}"),
        }
    }

    #[test]
    fn zip_members_lists_files_skipping_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let archive = fixture_archive(&store, dir.path(), "fonts.zip", &zip_bytes_with_dir());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["a.txt", "sub/b.txt"]),
            Err(error) => panic!("zip members list: {error}"),
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
            Err(error) => panic!("zip member resolves: {error}"),
        }
        match store.member(&archive, "a.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"alpha"),
            Err(error) => panic!("repeat member reuses the cache: {error}"),
        }
        assert!(
            !dir.path().join("temp").join("extract").exists(),
            "zip unpack leaves no extract folder"
        );
    }

    #[test]
    fn zip_high_mode_extracts_as_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let resources = test_resources(dir.path());
        let archive = fixture_archive(&store, dir.path(), "link.zip", &symlink_zip_bytes());
        match store.members(&archive) {
            Ok(names) => assert_eq!(names, vec!["link.txt"]),
            Err(error) => panic!("symlink members list: {error}"),
        }
        match store.member(&archive, "link.txt") {
            Ok(handle) => assert_eq!(open_text(&resources, &handle), b"a.txt"),
            Err(error) => panic!("symlink member resolves: {error}"),
        }
    }

    #[test]
    fn write_maps_quota() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let raw = tar_gz_bytes(&[("a.txt", b"plain")]);
        let archive = fixture_archive(&store, dir.path(), "note.tar.gz", &raw);
        let source = store.resources.cache_path(archive.sha256());
        let error = std::io::Error::new(std::io::ErrorKind::QuotaExceeded, "capped disk");
        match ArchiveError::from_write_io(&source, error) {
            ArchiveError::Write { path, fault } => {
                assert!(
                    matches!(fault, AccessFault::QuotaExceeded),
                    "mapping keeps the fault"
                );
                assert_eq!(path, source, "mapping keeps the source");
            }
            error => panic!("wrong mapping variant: {error}"),
        }
    }

    #[test]
    fn unpack_announces_unpacked_with_counts() {
        use confit_driver::progress::Event;

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
                assert_eq!(names.len(), 2, "unpack lists two members")
            }
            Err(error) => panic!("archive unpacks: {error}"),
        }
        let events: Vec<Event> = receiver.try_iter().collect();
        assert_eq!(events.len(), 1, "unpack fires one fact");
        match &events[0] {
            Event::Unpacked {
                archive: seen,
                total,
            } => {
                assert_eq!(*total, 2, "unpack counts every member");
                assert_eq!(
                    seen,
                    &dir.path()
                        .join("fonts.tar.gz")
                        .to_string_lossy()
                        .into_owned(),
                    "unpack names the archive path: {seen}"
                );
            }
            other => panic!("unpack announces itself: {other:?}"),
        }
    }

    #[test]
    fn unknown_algorithm_keeps_backend_detail() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fonts.zip");
        let detail = "compression method 99".to_owned();
        match Unpack::driver_error(
            &source,
            driver::archive::Error::UnknownAlgorithm {
                detail: detail.clone(),
            },
        ) {
            ArchiveError::UnknownAlgorithm { path, detail } => {
                assert_eq!(path, source);
                assert_eq!(detail, "compression method 99");
            }
            error => panic!("wrong unknown variant: {error}"),
        }
        let rendered = ArchiveError::UnknownAlgorithm {
            path: source.clone(),
            detail,
        }
        .to_string();
        assert!(
            rendered.contains(&source.to_string_lossy().into_owned()),
            "message shows the path: {rendered}"
        );
        assert!(
            rendered.contains("compression method 99"),
            "message shows the detail: {rendered}"
        );
    }

    #[test]
    fn unknown_failure_keeps_backend_detail() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fonts.zip");
        let detail = "mystery fault".to_owned();
        match Unpack::driver_error(
            &source,
            driver::archive::Error::Unknown {
                detail: detail.clone(),
            },
        ) {
            ArchiveError::Unknown { path, detail } => {
                assert_eq!(path, source);
                assert_eq!(detail, "mystery fault");
            }
            error => panic!("wrong unknown variant: {error}"),
        }
        let rendered = ArchiveError::Unknown {
            path: source.clone(),
            detail,
        }
        .to_string();
        assert!(
            rendered.contains(&source.to_string_lossy().into_owned()),
            "message shows the path: {rendered}"
        );
        assert!(
            rendered.contains("mystery fault"),
            "message shows the detail: {rendered}"
        );
        assert!(
            rendered.contains("please report steps to replicate"),
            "message asks for replication: {rendered}"
        );
    }

    #[test]
    fn stream_names_disk_kinds_before_corrupt() {
        use crate::faults::AccessFault;

        let source = Path::new("fonts.tar.gz");
        let error = from_stream(
            source,
            std::io::Error::new(std::io::ErrorKind::StorageFull, "disk failed"),
        );
        match &error {
            ArchiveError::Write { path: found, fault } => {
                assert_eq!(found, source, "extract keeps the source");
                assert!(
                    matches!(*fault, AccessFault::StorageFull),
                    "extract keeps the fault: {error}"
                );
            }
            other => panic!("wrong extract variant: {other}"),
        }
        let error = from_stream(
            source,
            std::io::Error::new(std::io::ErrorKind::InvalidData, "corrupt bytes"),
        );
        assert!(
            matches!(error, ArchiveError::CorruptedArchive { .. }),
            "corrupt bytes stay corrupt: {error}"
        );
    }
}
