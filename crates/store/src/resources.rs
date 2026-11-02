//! Trusted sources with a content-addressed cache.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::RwLock;

use confit_core::error::{
    ContextualCause, ContextualResult, ErrorCause, FilesystemError, WithContext, contextual,
};
use confit_core::resource_ref::ResourceRef;
use confit_core::sha::{Sha, ShaWriter};

use crate::StoreRoots;
use crate::pool::PoolStore;
use confit_driver as driver;

/// The folder that holds cache entries under the temp base.
const CACHE_DIR: &str = "cache";

/// The read-only mode for landed cache files.
const CACHE_MODE: u32 = 0o444;

/// A store of trusted project files and their content-addressed cache.
#[derive(Debug, Clone)]
pub struct Resources {
    /// The store roots.
    pub roots: StoreRoots,
    /// The pool behind cache promotion.
    pools: Arc<PoolStore>,
    /// The project path remembered for each validated hash.
    remembered: Arc<RwLock<HashMap<Sha, PathBuf>>>,
}

impl Resources {
    /// Builds a resource store under explicit roots.
    pub fn new(roots: &StoreRoots, pools: Arc<PoolStore>) -> Self {
        Self {
            roots: roots.clone(),
            pools,
            remembered: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Returns the folder that holds cache entries.
    pub fn cache_dir(&self) -> PathBuf {
        self.roots.temp_base.join(CACHE_DIR)
    }

    /// Returns the file path of one cache entry.
    pub fn cache_path(&self, sha: &Sha) -> PathBuf {
        self.cache_dir().join(sha.hex())
    }

    /// Returns the remembered project path for one content hash.
    fn remembered(&self, sha: &Sha) -> Option<PathBuf> {
        if let Ok(guard) = self.remembered.read()
            && let Some(path) = guard.get(sha)
        {
            return Some(path.clone());
        }
        None
    }

    /// Returns the validated hash for one trusted project file.
    ///
    /// - Remembers the project path for later resolution.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError::MalformedPath`] when the path is malformed.
    /// - [`FilesystemError::OutsideRoot`] when the path leaves the project root.
    /// - [`FilesystemError`] when the file is unreadable.
    pub fn resource(&self, relative: &str) -> ContextualResult<ResourceRef> {
        let parts = match split_checked(relative) {
            Ok(parts) => parts,
            Err(_) => {
                return Err(contextual(FilesystemError::MalformedPath));
            }
        };
        let exec_root = &self.roots.project_root;
        let mut full = exec_root.clone();
        for part in &parts {
            full.push(part);
        }
        if !normalize(&full).starts_with(normalize(exec_root)) {
            return Err(contextual(FilesystemError::OutsideRoot).with_fs_path(full));
        }
        let canonical_full = std::fs::canonicalize(&full)
            .map_err(ContextualCause::from)
            .with_fs_path(&full)?;
        let canonical_root = std::fs::canonicalize(exec_root)
            .map_err(ContextualCause::from)
            .with_fs_path(exec_root)?;
        if !canonical_full.starts_with(&canonical_root) {
            return Err(
                contextual(FilesystemError::OutsideRoot).with_canonical_path(canonical_full)
            );
        }
        let mut file = std::fs::File::open(&canonical_full)
            .map_err(ContextualCause::from)
            .with_canonical_path(&canonical_full)?;
        let sha = Sha::read(&mut file)
            .map_err(ContextualCause::from)
            .with_canonical_path(&canonical_full)?;
        let reference = ResourceRef::new(sha);
        if let Ok(mut guard) = self.remembered.write() {
            guard.insert(sha, canonical_full);
        }
        Ok(reference)
    }

    /// Resolves one content hash to its file path.
    ///
    /// - Returns the remembered project path for a project file.
    /// - Returns the cache path for a stored entry.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError::FileMissing`] when the hash holds no file.
    /// - [`ErrorCause::ContentHashMismatch`] when the pooled bytes fail.
    /// - [`FilesystemError`] when a cache write fails.
    pub fn resolve(&self, sha: &Sha) -> ContextualResult<PathBuf> {
        if let Some(path) = self.remembered(sha) {
            return Ok(path);
        }
        let dest = self.cache_path(sha);
        if !dest.exists() {
            self.promote(sha)?;
        }
        if dest.exists() {
            Ok(dest)
        } else {
            Err(contextual(FilesystemError::FileMissing).with_cache_path(dest))
        }
    }

    /// Stores the bytes from one reader under the expected hash.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the stream fails the expected hash.
    /// - [`FilesystemError`] when a cache write fails.
    pub fn store_checked(
        &self,
        sha: &Sha,
        mut reader: impl std::io::Read,
    ) -> ContextualResult<ResourceRef> {
        let dest = self.cache_path(sha);
        if dest.exists() {
            return Ok(ResourceRef::new(*sha));
        }
        let staged = driver::StagedFile::stage(&dest).with_cache_path(&dest)?;
        let pending = staged.path().to_path_buf();
        let found = {
            let file = std::fs::File::create(&pending)
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            let mut writer = ShaWriter::new(file);
            std::io::copy(&mut reader, &mut writer)
                .map_err(ContextualCause::from)
                .with_cache_path(&dest)?;
            writer.digest()
        };
        if found != *sha {
            return Err(contextual(ErrorCause::ContentHashMismatch {
                expected: *sha,
                found,
            })
            .with_cache_path(dest));
        }
        staged.persist(&dest).with_cache_path(&dest)?;
        driver::fs::set_mode(&dest, Some(CACHE_MODE)).with_cache_path(&dest)?;
        Ok(ResourceRef::new(*sha))
    }

    /// Stores the bytes from one reader under their content hash.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when a cache write fails.
    pub fn store(&self, mut reader: impl std::io::Read) -> ContextualResult<ResourceRef> {
        let land = self.cache_dir().join("land");
        let staged = driver::StagedFile::stage(&land).with_cache_path(self.cache_dir())?;
        let pending = staged.path().to_path_buf();
        let found = {
            let file = std::fs::File::create(&pending)
                .map_err(ContextualCause::from)
                .with_cache_path(self.cache_dir())?;
            let mut writer = ShaWriter::new(file);
            std::io::copy(&mut reader, &mut writer)
                .map_err(ContextualCause::from)
                .with_cache_path(self.cache_dir())?;
            writer.digest()
        };
        let dest = self.cache_path(&found);
        staged.persist(&dest).with_cache_path(&dest)?;
        driver::fs::set_mode(&dest, Some(CACHE_MODE)).with_cache_path(&dest)?;
        Ok(ResourceRef::new(found))
    }

    /// Reads one trusted file and decodes its UTF-8 text.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the file is unreadable.
    /// - [`DocumentError::ParseFailed`] when the file holds non-UTF8 bytes.
    pub fn read_text(&self, sha: &ResourceRef) -> ContextualResult<String> {
        let bytes = self.read_bytes(sha)?;
        String::from_utf8(bytes).map_err(|error| {
            let message = error.to_string();
            match self.remembered(sha.sha()) {
                Some(path) => ContextualCause::from(error)
                    .with_fs_path(path)
                    .with_detail(message),
                None => ContextualCause::from(error)
                    .with_cache_path(self.cache_path(sha.sha()))
                    .with_detail(message),
            }
        })
    }

    /// Opens one trusted file for reading.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the file is unreadable.
    pub fn open(&self, sha: &ResourceRef) -> ContextualResult<std::fs::File> {
        match self.remembered(sha.sha()) {
            Some(path) => std::fs::File::open(&path)
                .map_err(ContextualCause::from)
                .with_fs_path(&path),
            None => self.open_cache(sha.sha()),
        }
    }

    /// Opens one cache entry and fills it from the pool when it is absent.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the promoted hash mismatches.
    /// - [`FilesystemError`] when the cache entry is unreadable.
    fn open_cache(&self, sha: &Sha) -> ContextualResult<std::fs::File> {
        let dest = self.cache_path(sha);
        if !dest.exists() {
            self.promote(sha)?;
        }
        std::fs::File::open(&dest)
            .map_err(ContextualCause::from)
            .with_cache_path(dest)
    }

    /// Copies one pool entry into the cache.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the stream fails the expected hash.
    /// - [`FilesystemError`] when the pool entry is unreadable.
    fn promote(&self, sha: &Sha) -> ContextualResult<()> {
        let reader = self.pools.open(sha)?;
        self.store_checked(sha, reader)?;
        Ok(())
    }

    /// Copies one trusted file into the pool.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] when the stream fails the expected hash.
    /// - [`FilesystemError`] when a pool or cache read fails.
    pub fn persist(&self, sha: &ResourceRef) -> ContextualResult<Sha> {
        if self.pools.path(sha.sha()).exists() {
            return Ok(*sha.sha());
        }
        let mut reader = self.open(sha)?;
        self.pools.store_checked(sha.sha(), &mut reader)?;
        Ok(*sha.sha())
    }

    /// Reads one trusted file into memory.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the file is unreadable.
    pub fn read_bytes(&self, sha: &ResourceRef) -> ContextualResult<Vec<u8>> {
        use std::io::Read as _;

        match self.remembered(sha.sha()) {
            Some(path) => std::fs::read(&path)
                .map_err(ContextualCause::from)
                .with_fs_path(&path),
            None => {
                let mut stream = self.open(sha)?;
                let mut bytes = Vec::new();
                stream
                    .read_to_end(&mut bytes)
                    .map_err(ContextualCause::from)
                    .with_cache_path(self.cache_path(sha.sha()))?;
                Ok(bytes)
            }
        }
    }
}

/// Splits one relative path into checked components.
fn split_checked(raw: &str) -> std::result::Result<Vec<String>, String> {
    if raw.contains('\\') {
        return Err("expected a path without backslashes".to_owned());
    }
    if raw.len() >= 2 && raw.as_bytes()[0].is_ascii_alphabetic() && raw.as_bytes()[1] == b':' {
        return Err("expected a relative path".to_owned());
    }
    if raw.starts_with('/') {
        return Err("expected a relative path".to_owned());
    }
    let mut parts = Vec::new();
    for component in raw.split('/') {
        if component.is_empty() {
            return Err("expected a path without empty components".to_owned());
        }
        if component == ".." {
            return Err("expected a path without parent steps".to_owned());
        }
        parts.push(component.to_owned());
    }
    if parts.is_empty() {
        return Err("expected a path".to_owned());
    }
    Ok(parts)
}

/// Resolves the `.` and `..` components of one path.
fn normalize(path: &Path) -> PathBuf {
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
    use confit_core::error::ErrorCause;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            temp_base: dir.join("temp"),
            project_root: dir.join("project"),
        }
    }

    fn test_pair(dir: &Path) -> (Resources, Arc<PoolStore>) {
        let roots = test_roots(dir);
        let (sender, _) = crossbeam_channel::unbounded();
        let pools = Arc::new(PoolStore::new(&roots, sender));
        (Resources::new(&roots, pools.clone()), pools)
    }

    fn test_store(dir: &Path) -> Resources {
        test_pair(dir).0
    }

    fn project_file(dir: &Path, name: &str, body: &[u8]) -> PathBuf {
        let root = dir.join("project");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    fn canonical(dir: &Path, name: &str) -> PathBuf {
        std::fs::canonicalize(dir.join("project").join(name)).unwrap()
    }

    fn cache_path(dir: &Path, sha: &Sha) -> PathBuf {
        dir.join("temp").join("cache").join(sha.hex())
    }

    #[test]
    fn resource_opens_a_relative_file() {
        let dir = tempfile::tempdir().unwrap();
        project_file(dir.path(), "profile.lua", b"return {}");
        let store = test_store(dir.path());
        match store.resource("profile.lua") {
            Ok(handle) => {
                assert_eq!(handle.sha(), &Sha::hash(b"return {}"));
                match store.resolve(handle.sha()) {
                    Ok(found) => assert_eq!(found, canonical(dir.path(), "profile.lua")),
                    Err(error) => panic!("the trusted path fails to resolve: {error}"),
                }
                match store.read_text(&handle) {
                    Ok(found) => assert_eq!(found, "return {}"),
                    Err(error) => panic!("the trusted text fails to load: {error}"),
                }
            }
            Err(error) => panic!("the resource fails to create: {error}"),
        }
    }

    #[test]
    fn resource_rejects_escaped_and_empty_paths() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        let store = test_store(dir.path());
        for input in ["../outside.lua", "/etc/passwd", "", "sub\\note.lua"] {
            assert!(
                store.resource(input).is_err(),
                "the resource rejects the path: {input}"
            );
        }
    }

    #[test]
    fn resource_names_an_absent_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        let store = test_store(dir.path());
        match store.resource("absent.lua") {
            Ok(_) => panic!("expected an absent file"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent file keeps its cause: {error}"
                );
                match &error.context.fs_path {
                    Some(found) => assert_eq!(
                        found,
                        &dir.path().join("project").join("absent.lua"),
                        "the absent file keeps the path"
                    ),
                    None => panic!("expected the file path: {error}"),
                }
            }
        }
    }

    #[test]
    fn resource_rejects_an_outside_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("escaped.txt");
        std::fs::write(&outside, b"secret").unwrap();
        driver::fs::write_link(&root.join("outside"), &outside).unwrap();
        let store = test_store(dir.path());
        match store.resource("outside") {
            Ok(_) => panic!("expected an escaping link"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::OutsideRoot)
                    ),
                    "the escaping link keeps the outside-root cause: {error}"
                );
                match &error.context.canonical_path {
                    Some(_) => {}
                    None => panic!("expected the canonical path: {error}"),
                }
            }
        }
    }

    #[test]
    fn resource_follows_an_inside_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("target.txt");
        std::fs::write(&target, b"inside bytes").unwrap();
        driver::fs::write_link(&root.join("inside"), Path::new("target.txt")).unwrap();
        let store = test_store(dir.path());
        match store.resource("inside") {
            Ok(handle) => {
                match store.resolve(handle.sha()) {
                    Ok(found) => assert_eq!(found, canonical(dir.path(), "target.txt")),
                    Err(error) => panic!("the link fails to resolve: {error}"),
                }
                match store.read_text(&handle) {
                    Ok(found) => assert_eq!(found, "inside bytes"),
                    Err(error) => panic!("the link text fails to load: {error}"),
                }
            }
            Err(error) => panic!("the inside link fails to create: {error}"),
        }
    }

    #[test]
    fn resolve_serves_the_cache_without_a_project_path() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"alpha");
        let handle = match store.store(std::io::Cursor::new(b"alpha")) {
            Ok(found) => found,
            Err(error) => panic!("the cache fails to store: {error}"),
        };
        assert_eq!(handle, ResourceRef::new(sha));
        match store.resolve(&sha) {
            Ok(found) => assert_eq!(found, cache_path(dir.path(), &sha)),
            Err(error) => panic!("the cache fails to resolve: {error}"),
        }
    }

    #[test]
    fn resolve_names_an_absent_hash() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"absent");
        match store.resolve(&sha) {
            Ok(_) => panic!("expected an absent hash"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent hash keeps its cause: {error}"
                );
                match &error.context.cache_path {
                    Some(found) => assert_eq!(
                        found,
                        &pools.path(&sha),
                        "the absent hash keeps the pool path"
                    ),
                    None => panic!("expected the pool path: {error}"),
                }
            }
        }
    }

    #[test]
    fn read_text_rejects_invalid_text() {
        use confit_core::error::{DocumentError, Require};

        let dir = tempfile::tempdir().unwrap();
        let path = project_file(dir.path(), "binary.lua", &[0xFF, 0xFE, b'x']);
        let store = test_store(dir.path());
        match store.resource("binary.lua") {
            Ok(handle) => match store.read_text(&handle) {
                Ok(_) => panic!("expected invalid text"),
                Err(error) => {
                    match store.resolve(handle.sha()) {
                        Ok(found) => assert_eq!(found, path),
                        Err(fault) => panic!("the binary path fails to resolve: {fault}"),
                    }
                    assert!(
                        matches!(
                            error.cause(),
                            ErrorCause::DocumentError(DocumentError::ParseFailed {
                                require: Require::Utf8Text,
                            })
                        ),
                        "the binary file keeps its text cause: {error}"
                    );
                    match &error.context.fs_path {
                        Some(found) => assert_eq!(found, &path, "the binary file keeps the path"),
                        None => panic!("expected the file path: {error}"),
                    }
                }
            },
            Err(error) => panic!("the binary resource fails to create: {error}"),
        }
    }

    #[test]
    fn store_lands_bytes_read_only() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"alpha");
        let handle = match store.store(std::io::Cursor::new(b"alpha")) {
            Ok(handle) => handle,
            Err(error) => panic!("the cache fails to store: {error}"),
        };
        assert_eq!(handle, ResourceRef::new(sha));
        let landed = cache_path(dir.path(), &sha);
        assert_eq!(std::fs::read(&landed).unwrap(), b"alpha");
        match driver::fs::mode(&landed) {
            Ok(mode) => assert_eq!(mode, 0o444, "the landed cache entry is read-only"),
            Err(error) => panic!("the landed mode fails to read: {error}"),
        }
        let mut found = Vec::new();
        match store.open(&handle) {
            Ok(mut reader) => {
                reader.read_to_end(&mut found).unwrap();
            }
            Err(error) => panic!("the cached bytes fail to open: {error}"),
        }
        assert_eq!(found, b"alpha");
    }

    #[test]
    fn store_skips_present_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"alpha");
        match store.store_checked(&sha, std::io::Cursor::new(b"alpha")) {
            Ok(_) => {}
            Err(error) => panic!("the cache fails to store: {error}"),
        }
        match store.store_checked(&sha, FailRead) {
            Ok(handle) => assert_eq!(handle, ResourceRef::new(sha)),
            Err(error) => panic!("the present cache fails to store: {error}"),
        }
        assert_eq!(
            std::fs::read(cache_path(dir.path(), &sha)).unwrap(),
            b"alpha",
            "the present bytes match"
        );
    }

    /// A test reader that fails every read.
    struct FailRead;

    impl std::io::Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("the stream is not read"))
        }
    }

    #[test]
    fn store_rejects_a_mismatched_stream() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"alpha");
        let other_sha = Sha::hash(b"garbage");
        match store.store_checked(&sha, std::io::Cursor::new(b"garbage")) {
            Ok(_) => panic!("expected a mismatched stream"),
            Err(error) => match error.cause() {
                ErrorCause::ContentHashMismatch { expected, found } => {
                    assert_eq!(expected, &sha, "the mismatch keeps the expected hash");
                    assert_eq!(found, &other_sha, "the mismatch keeps the found hash");
                    match &error.context.cache_path {
                        Some(found) => assert_eq!(
                            found,
                            &cache_path(dir.path(), &sha),
                            "the mismatch keeps the cache path"
                        ),
                        None => panic!("expected the cache path: {error}"),
                    }
                }
                other => panic!("expected a content hash mismatch: {other:?}"),
            },
        }
        assert!(
            !cache_path(dir.path(), &sha).exists(),
            "the mismatch leaves no cache file"
        );
    }

    #[test]
    fn open_names_an_absent_cache() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"absent");
        let handle = ResourceRef::new(sha);
        match store.open(&handle) {
            Ok(_) => panic!("expected an absent cache"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent cache keeps its cause: {error}"
                );
                match &error.context.cache_path {
                    Some(found) => assert_eq!(
                        found,
                        &pools.path(&sha),
                        "the absent cache keeps the cache path"
                    ),
                    None => panic!("expected the cache path: {error}"),
                }
            }
        }
    }

    #[test]
    fn open_promotes_pool_bytes_into_cache() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        match pools.store_checked(&sha, &b"pooled bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        let handle = ResourceRef::new(sha);
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"pooled bytes");
            }
            Err(error) => panic!("the promoted bytes fail to open: {error}"),
        }
        let landed = cache_path(dir.path(), &sha);
        assert!(landed.exists(), "the promotion lands the cache file");
        match driver::fs::mode(&landed) {
            Ok(mode) => assert_eq!(mode, 0o444, "the promoted cache entry is read-only"),
            Err(error) => panic!("the promoted mode fails to read: {error}"),
        }
    }

    #[test]
    fn open_rejects_renamed_pool_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        match pools.store_checked(&sha, &b"pooled bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        let other_sha = Sha::hash(b"other bytes");
        std::fs::rename(pools.path(&sha), pools.path(&other_sha)).unwrap();
        let handle = ResourceRef::new(other_sha);
        match store.open(&handle) {
            Ok(_) => panic!("expected a renamed entry error"),
            Err(error) => match error.cause() {
                ErrorCause::ContentHashMismatch { expected, .. } => {
                    assert_eq!(expected, &other_sha, "the mismatch keeps the expected hash");
                }
                other => panic!("expected a content hash mismatch: {other}"),
            },
        }
    }

    #[test]
    fn open_tracks_project_bytes() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let path = project_file(dir.path(), "note.txt", b"hello");
        let store = test_store(dir.path());
        let handle = match store.resource("note.txt") {
            Ok(handle) => handle,
            Err(error) => panic!("the resource fails to create: {error}"),
        };
        std::fs::write(&path, b"drifted").unwrap();
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"drifted", "the open reads the drifted bytes");
            }
            Err(error) => panic!("the project bytes fail to open: {error}"),
        }
    }

    #[test]
    fn persist_cache_fills_pool() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"cached bytes");
        match store.store_checked(&sha, std::io::Cursor::new(b"cached bytes")) {
            Ok(_) => {}
            Err(error) => panic!("the cache fails to store: {error}"),
        }
        assert!(pools.open(&sha).is_err(), "the pool holds no entry");
        match store.persist(&ResourceRef::new(sha)) {
            Ok(found) => assert_eq!(found, sha),
            Err(error) => panic!("the cache fails to persist: {error}"),
        }
        match pools.open(&sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"cached bytes");
            }
            Err(error) => panic!("the persisted bytes fail to open: {error}"),
        }
    }

    #[test]
    fn persist_skips_present_pool_entries() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        match pools.store_checked(&sha, &b"pooled bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        match store.persist(&ResourceRef::new(sha)) {
            Ok(found) => assert_eq!(found, sha, "the present pool returns the digest"),
            Err(error) => panic!("the present pool fails to persist: {error}"),
        }
    }

    #[test]
    fn persist_names_an_absent_pool_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"absent");
        match store.persist(&ResourceRef::new(sha)) {
            Ok(_) => panic!("expected an absent cache"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "the absent cache keeps its cause: {error}"
                );
                match &error.context.cache_path {
                    Some(found) => assert_eq!(
                        found,
                        &pools.path(&sha),
                        "the absent entry keeps the pool path"
                    ),
                    None => panic!("expected the pool path: {error}"),
                }
            }
        }
    }

    #[test]
    fn persist_names_pool_path_on_pool_fault() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"hello");
        match store.store_checked(&sha, std::io::Cursor::new(b"hello")) {
            Ok(_) => {}
            Err(error) => panic!("the cache fails to store: {error}"),
        }
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config").join("blobs"), b"not a dir").unwrap();
        match store.persist(&ResourceRef::new(sha)) {
            Ok(_) => panic!("expected a blocked pool"),
            Err(error) => match &error.context.cache_path {
                Some(found) => assert_eq!(
                    found,
                    &pools.path(&sha),
                    "the blocked pool keeps the cache path"
                ),
                None => panic!("expected the pool path: {error}"),
            },
        }
    }
}
