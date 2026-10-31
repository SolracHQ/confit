//! Resources trusted sources and text.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use confit_core::error::{
    ContextualCause, ContextualResult, ErrorCause, FilesystemError, WithContext, contextual,
};
use confit_core::sha::{Sha, ShaWriter};
use file_format::FileFormat;
use mlua::{Lua, Table};

use crate::StoreRoots;
use crate::handles::ResourceHandle;
use crate::pool::PoolStore;
use confit_driver as driver;

/// Content-addressed cache folder under the temp base.
const CACHE_DIR: &str = "cache";

/// Read-only bits for landed cache files.
const CACHE_MODE: u32 = 0o444;

/// Trusted project files with a content-addressed cache.
#[derive(Debug, Clone)]
pub struct Resources {
    /// The store roots for construction.
    pub roots: StoreRoots,
    /// The pool backing cache promotion.
    pools: Arc<PoolStore>,
}

impl Resources {
    /// Build file-backed resources under explicit roots.
    pub fn new(roots: &StoreRoots, pools: Arc<PoolStore>) -> Self {
        Self {
            roots: roots.clone(),
            pools,
        }
    }

    /// Derive the cache folder holding landed entries.
    pub fn cache_dir(&self) -> PathBuf {
        self.roots.temp_base.join(CACHE_DIR)
    }

    /// Derive the cache path for one content hash.
    pub fn cache_path(&self, sha: &Sha) -> PathBuf {
        self.cache_dir().join(sha.hex())
    }

    fn resolved_path(&self, handle: &ResourceHandle) -> PathBuf {
        match handle {
            ResourceHandle::Project(path, _) => path.clone(),
            ResourceHandle::Cache(sha) => self.cache_path(sha),
        }
    }

    /// Render one handle for error text.
    fn display(&self, handle: &ResourceHandle) -> String {
        self.resolved_path(handle).display().to_string()
    }

    /// Create a resource handle for one project-relative file.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError::MalformedPath`] for malformed inputs.
    /// - [`FilesystemError::OutsideRoot`] for escaping inputs.
    /// - [`FilesystemError`] for unreadable files.
    pub fn resource(&self, relative: &str) -> ContextualResult<ResourceHandle> {
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
        Ok(ResourceHandle::Project(canonical_full, sha))
    }

    /// Land reader bytes in the cache under the expected hash.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for streams failing the expected hash.
    /// - [`FilesystemError`] for cache write faults.
    pub fn store_checked(
        &self,
        sha: &Sha,
        mut reader: impl std::io::Read,
    ) -> ContextualResult<ResourceHandle> {
        let dest = self.cache_path(sha);
        if dest.exists() {
            return Ok(ResourceHandle::Cache(sha.clone()));
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
                expected: sha.clone(),
                found,
            })
            .with_cache_path(dest));
        }
        staged.persist(&dest).with_cache_path(&dest)?;
        driver::fs::set_mode(&dest, Some(CACHE_MODE)).with_cache_path(&dest)?;
        Ok(ResourceHandle::Cache(sha.clone()))
    }

    /// Land reader bytes in the cache under the discovered hash.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for cache write faults.
    pub fn store(&self, mut reader: impl std::io::Read) -> ContextualResult<ResourceHandle> {
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
        Ok(ResourceHandle::Cache(found))
    }

    /// Read one trusted file as text.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for unreadable files.
    /// - [`DocumentError::ParseFailed`] for non-UTF8 files.
    pub fn read_text(&self, handle: &ResourceHandle) -> ContextualResult<String> {
        let bytes = self.read_bytes(handle)?;
        String::from_utf8(bytes).map_err(|error| {
            let message = error.to_string();
            match handle {
                ResourceHandle::Project(path, _) => ContextualCause::from(error)
                    .with_fs_path(path)
                    .with_detail(message),
                ResourceHandle::Cache(sha) => ContextualCause::from(error)
                    .with_cache_path(self.cache_path(sha))
                    .with_detail(message),
            }
        })
    }

    /// Stream one handle for reading.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for unreadable files.
    pub fn open(&self, handle: &ResourceHandle) -> ContextualResult<std::fs::File> {
        match handle {
            ResourceHandle::Project(path, _) => std::fs::File::open(path)
                .map_err(ContextualCause::from)
                .with_fs_path(path),
            ResourceHandle::Cache(sha) => self.open_cache(sha),
        }
    }

    /// Open one cache entry, promoting from the pool on miss.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for promoted hash mismatches.
    /// - [`FilesystemError`] for unreadable entries.
    fn open_cache(&self, sha: &Sha) -> ContextualResult<std::fs::File> {
        let dest = self.cache_path(sha);
        if !dest.exists() {
            self.promote(sha)?;
        }
        std::fs::File::open(&dest)
            .map_err(ContextualCause::from)
            .with_cache_path(dest)
    }

    /// Promote one pool entry into the cache.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for streams failing the expected hash.
    /// - [`FilesystemError`] for unreadable pool entries.
    fn promote(&self, sha: &Sha) -> ContextualResult<()> {
        let reader = self.pools.open(sha)?;
        self.store_checked(sha, reader)?;
        Ok(())
    }

    /// Fill the pool from one stored digest.
    ///
    /// # Errors
    ///
    /// - [`ErrorCause::ContentHashMismatch`] for streams failing the expected hash.
    /// - [`FilesystemError`] for unreadable cache and pool write faults.
    pub fn persist(&self, sha: &Sha) -> ContextualResult<Sha> {
        if self.pools.path(sha).exists() {
            return Ok(sha.clone());
        }
        let dest = self.cache_path(sha);
        let mut reader = std::fs::File::open(&dest)
            .map_err(ContextualCause::from)
            .with_cache_path(&dest)?;
        self.pools.store_checked(sha, &mut reader)?;
        Ok(sha.clone())
    }

    /// Read one handle fully into bytes.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for unreadable files.
    fn read_bytes(&self, handle: &ResourceHandle) -> ContextualResult<Vec<u8>> {
        use std::io::Read as _;

        match handle {
            ResourceHandle::Project(path, _) => std::fs::read(path)
                .map_err(ContextualCause::from)
                .with_fs_path(path),
            ResourceHandle::Cache(sha) => {
                let mut stream = self.open(handle)?;
                let mut bytes = Vec::new();
                stream
                    .read_to_end(&mut bytes)
                    .map_err(ContextualCause::from)
                    .with_cache_path(self.cache_path(sha))?;
                Ok(bytes)
            }
        }
    }

    /// Load one handle payload into its Lua table.
    pub fn load_table(
        &self,
        lua: &Lua,
        handle: &ResourceHandle,
    ) -> std::result::Result<Table, String> {
        match sniffed_bytes(self, handle) {
            Ok(bytes) => table_for(lua, self, handle, &bytes),
            Err(reason) => Err(reason),
        }
    }
}

/// Split one segment string into checked components.
///
/// # Errors
///
/// - Returns the escape reason for every escaping or empty shape.
fn split_checked(raw: &str) -> std::result::Result<Vec<String>, String> {
    if raw.contains('\\') {
        return Err("backslashes are not supported".to_owned());
    }
    if raw.len() >= 2 && raw.as_bytes()[0].is_ascii_alphabetic() && raw.as_bytes()[1] == b':' {
        return Err("drive prefixes are not supported".to_owned());
    }
    if raw.starts_with('/') {
        return Err("expected a relative path".to_owned());
    }
    let mut parts = Vec::new();
    for component in raw.split('/') {
        if component.is_empty() {
            return Err("empty components are not supported".to_owned());
        }
        if component == ".." {
            return Err("parent steps are not supported".to_owned());
        }
        parts.push(component.to_owned());
    }
    if parts.is_empty() {
        return Err("expected a path".to_owned());
    }
    Ok(parts)
}

/// Normalize one path lexically.
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

/// Read one handle fully into bytes.
fn sniffed_bytes(
    resources: &Resources,
    handle: &ResourceHandle,
) -> std::result::Result<Vec<u8>, String> {
    let path = resources.display(handle);
    let bytes = resources
        .read_bytes(handle)
        .map_err(|error| error.to_string())?;
    match FileFormat::from_reader(std::io::Cursor::new(bytes.as_slice())) {
        Ok(FileFormat::PlainText) => Ok(bytes),
        Ok(format) => Err(format!(
            "cannot load '{path}': unsupported kind '{}'",
            format.name()
        )),
        Err(error) => Err(format!("cannot sniff '{path}': {error}")),
    }
}

/// Read one handle payload into its Lua table.
fn table_for(
    lua: &Lua,
    resources: &Resources,
    handle: &ResourceHandle,
    bytes: &[u8],
) -> std::result::Result<Table, String> {
    use mlua::Value;
    let path = resources.display(handle);
    let text = match std::str::from_utf8(bytes) {
        Ok(found) => found,
        Err(_) => return Err(format!("cannot load '{path}'")),
    };
    if let Some(value) = parse_json(text)
        .or_else(|| parse_toml(text))
        .or_else(|| parse_yaml(text))
        .or_else(|| parse_ini(text))
    {
        use mlua::LuaSerdeExt as _;
        match lua.to_value(&value) {
            Ok(Value::Table(table)) => return Ok(table),
            _ => return Err(format!("cannot load '{path}'")),
        }
    }
    Err(format!("cannot load '{path}'"))
}

/// Read one JSON payload with its object shape.
fn parse_json(text: &str) -> Option<serde_json::Value> {
    match serde_json::from_str::<serde_json::Value>(text).ok()? {
        serde_json::Value::Object(_) => serde_json::from_str(text).ok(),
        _ => None,
    }
}

/// Read one TOML payload with its object shape.
fn parse_toml(text: &str) -> Option<serde_json::Value> {
    let found = toml::from_str::<toml::Value>(text).ok()?;
    match serde_json::to_value(&found).ok()? {
        serde_json::Value::Object(_) => serde_json::to_value(&found).ok(),
        _ => None,
    }
}

/// Read one YAML payload with its object shape.
fn parse_yaml(text: &str) -> Option<serde_json::Value> {
    let found = noyalib::from_str::<noyalib::Value>(text).ok()?;
    match serde_json::to_value(&found).ok()? {
        serde_json::Value::Object(_) => serde_json::to_value(&found).ok(),
        _ => None,
    }
}

/// Read one INI payload with its object shape.
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

    fn cache_path(dir: &Path, sha: &Sha) -> PathBuf {
        dir.join("temp").join("cache").join(sha.hex())
    }

    #[test]
    fn resource_opens_relative_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = project_file(dir.path(), "profile.lua", b"return {}");
        let store = test_store(dir.path());
        match store.resource("profile.lua") {
            Ok(handle) => {
                assert_eq!(store.resolved_path(&handle), path.as_path());
                assert!(matches!(handle, ResourceHandle::Project(..)));
                match store.read_text(&handle) {
                    Ok(found) => assert_eq!(found, "return {}"),
                    Err(error) => panic!("trusted text load: {error}"),
                }
            }
            Err(error) => panic!("resource creates: {error}"),
        }
    }

    #[test]
    fn resource_rejects_foreign_inputs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        let store = test_store(dir.path());
        for input in ["../outside.lua", "/etc/passwd", "", "sub\\note.lua"] {
            assert!(store.resource(input).is_err(), "foreign input fails");
        }
    }

    #[test]
    fn resource_names_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("project")).unwrap();
        let store = test_store(dir.path());
        match store.resource("absent.lua") {
            Ok(_) => panic!("absent resource passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "absent keeps the cause: {error}"
                );
                match &error.context.fs_path {
                    Some(found) => assert_eq!(
                        found,
                        &dir.path().join("project").join("absent.lua"),
                        "absent keeps the path"
                    ),
                    None => panic!("absent keeps the path: {error}"),
                }
            }
        }
    }

    #[test]
    fn resource_rejected_outside_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let outside = dir.path().join("escaped.txt");
        std::fs::write(&outside, b"secret").unwrap();
        driver::fs::write_link(&root.join("outside"), &outside).unwrap();
        let store = test_store(dir.path());
        match store.resource("outside") {
            Ok(_) => panic!("escaping link passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::OutsideRoot)
                    ),
                    "escape keeps outside-root: {error}"
                );
                match &error.context.canonical_path {
                    Some(_) => {}
                    None => panic!("escape keeps the canonical path: {error}"),
                }
            }
        }
    }

    #[test]
    fn resource_follows_inside_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let target = root.join("target.txt");
        std::fs::write(&target, b"inside bytes").unwrap();
        driver::fs::write_link(&root.join("inside"), Path::new("target.txt")).unwrap();
        let store = test_store(dir.path());
        match store.resource("inside") {
            Ok(handle) => {
                assert_eq!(store.resolved_path(&handle), target.as_path());
                match store.read_text(&handle) {
                    Ok(found) => assert_eq!(found, "inside bytes"),
                    Err(error) => panic!("link text load: {error}"),
                }
            }
            Err(error) => panic!("inside link creates: {error}"),
        }
    }

    #[test]
    fn read_text_rejected_invalid_text() {
        use confit_core::error::{DocumentError, Require};

        let dir = tempfile::tempdir().unwrap();
        let path = project_file(dir.path(), "binary.lua", &[0xFF, 0xFE, b'x']);
        let store = test_store(dir.path());
        match store.resource("binary.lua") {
            Ok(handle) => match store.read_text(&handle) {
                Ok(_) => panic!("invalid text passes"),
                Err(error) => {
                    assert_eq!(store.resolved_path(&handle), path);
                    assert!(
                        matches!(
                            error.cause(),
                            ErrorCause::DocumentError(DocumentError::ParseFailed {
                                require: Require::Utf8Text,
                            })
                        ),
                        "binary keeps its text: {error}"
                    );
                    match &error.context.fs_path {
                        Some(found) => assert_eq!(found, &path, "binary keeps the path"),
                        None => panic!("binary keeps the path: {error}"),
                    }
                }
            },
            Err(error) => panic!("binary resource creates: {error}"),
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
            Err(error) => panic!("cache stores: {error}"),
        };
        assert_eq!(handle, ResourceHandle::Cache(sha.clone()));
        let landed = cache_path(dir.path(), &sha);
        assert_eq!(std::fs::read(&landed).unwrap(), b"alpha");
        match driver::fs::mode(&landed) {
            Ok(mode) => assert_eq!(mode, 0o444, "landed cache stays read-only"),
            Err(error) => panic!("landed modes: {error}"),
        }
        let mut found = Vec::new();
        match store.open(&handle) {
            Ok(mut reader) => {
                reader.read_to_end(&mut found).unwrap();
            }
            Err(error) => panic!("cached bytes open: {error}"),
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
            Err(error) => panic!("cache stores: {error}"),
        }
        match store.store_checked(&sha, FailRead) {
            Ok(handle) => assert_eq!(handle, ResourceHandle::Cache(sha.clone())),
            Err(error) => panic!("present cache skips: {error}"),
        }
        assert_eq!(
            std::fs::read(cache_path(dir.path(), &sha)).unwrap(),
            b"alpha",
            "present bytes stay"
        );
    }

    /// Reader failing on every read.
    struct FailRead;

    impl std::io::Read for FailRead {
        fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("stream stays unread"))
        }
    }

    #[test]
    fn reject_mismatched_stream() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"alpha");
        let wrong = Sha::hash(b"garbage");
        match store.store_checked(&sha, std::io::Cursor::new(b"garbage")) {
            Ok(_) => panic!("mismatched stream passes"),
            Err(error) => match error.cause() {
                ErrorCause::ContentHashMismatch { expected, found } => {
                    assert_eq!(expected, &sha, "mismatch keeps expected");
                    assert_eq!(found, &wrong, "mismatch keeps found");
                    match &error.context.cache_path {
                        Some(found) => assert_eq!(
                            found,
                            &cache_path(dir.path(), &sha),
                            "mismatch keeps the path"
                        ),
                        None => panic!("mismatch keeps the path: {error}"),
                    }
                }
                other => panic!("wrong mismatch cause: {other:?}"),
            },
        }
        assert!(
            !cache_path(dir.path(), &sha).exists(),
            "mismatch leaves no file"
        );
    }

    #[test]
    fn open_names_missing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"absent");
        let handle = ResourceHandle::Cache(sha.clone());
        match store.open(&handle) {
            Ok(_) => panic!("absent cache passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "absent keeps the cause: {error}"
                );
                match &error.context.cache_path {
                    Some(found) => {
                        assert_eq!(found, &pools.path(&sha), "absent keeps the cache path")
                    }
                    None => panic!("absent keeps the cache path: {error}"),
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
            Err(error) => panic!("pool stores: {error}"),
        }
        let handle = ResourceHandle::Cache(sha.clone());
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"pooled bytes");
            }
            Err(error) => panic!("promoted bytes open: {error}"),
        }
        let landed = cache_path(dir.path(), &sha);
        assert!(landed.exists(), "promotion lands the cache file");
        match driver::fs::mode(&landed) {
            Ok(mode) => assert_eq!(mode, 0o444, "promoted cache stays read-only"),
            Err(error) => panic!("promoted modes: {error}"),
        }
    }

    #[test]
    fn open_rejects_renamed_pool_entry() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        match pools.store_checked(&sha, &b"pooled bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        let wrong = Sha::hash(b"other bytes");
        std::fs::rename(pools.path(&sha), pools.path(&wrong)).unwrap();
        let handle = ResourceHandle::Cache(wrong.clone());
        match store.open(&handle) {
            Ok(_) => panic!("renamed entry opens"),
            Err(error) => match error.cause() {
                ErrorCause::ContentHashMismatch { expected, .. } => {
                    assert_eq!(expected, &wrong, "mismatch keeps expected");
                }
                other => panic!("wrong rename cause: {other}"),
            },
        }
    }

    #[test]
    fn open_tracks_live_project_bytes() {
        use std::io::Read as _;

        let dir = tempfile::tempdir().unwrap();
        let path = project_file(dir.path(), "note.txt", b"hello");
        let store = test_store(dir.path());
        let handle = match store.resource("note.txt") {
            Ok(handle) => handle,
            Err(error) => panic!("resource creates: {error}"),
        };
        std::fs::write(&path, b"drifted").unwrap();
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"drifted", "open skips verification");
            }
            Err(error) => panic!("live bytes open: {error}"),
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
            Err(error) => panic!("cache stores: {error}"),
        }
        assert!(pools.open(&sha).is_err(), "pool starts empty");
        match store.persist(&sha) {
            Ok(found) => assert_eq!(found, sha),
            Err(error) => panic!("cache persists: {error}"),
        }
        match pools.open(&sha) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, b"cached bytes");
            }
            Err(error) => panic!("persisted bytes open: {error}"),
        }
    }

    #[test]
    fn persist_skips_present_pool_entries() {
        let dir = tempfile::tempdir().unwrap();
        let (store, pools) = test_pair(dir.path());
        let sha = Sha::hash(b"pooled bytes");
        match pools.store_checked(&sha, &b"pooled bytes"[..]) {
            Ok(()) => {}
            Err(error) => panic!("pool stores: {error}"),
        }
        match store.persist(&sha) {
            Ok(found) => assert_eq!(found, sha, "present pool returns the digest"),
            Err(error) => panic!("present pool passes: {error}"),
        }
    }

    #[test]
    fn persist_names_missing_cache() {
        let dir = tempfile::tempdir().unwrap();
        let store = test_store(dir.path());
        let sha = Sha::hash(b"absent");
        match store.persist(&sha) {
            Ok(_) => panic!("absent cache passes"),
            Err(error) => {
                assert!(
                    matches!(
                        error.cause(),
                        ErrorCause::FilesystemError(FilesystemError::FileMissing)
                    ),
                    "absent keeps the cause: {error}"
                );
                match &error.context.cache_path {
                    Some(found) => assert_eq!(
                        found,
                        &cache_path(dir.path(), &sha),
                        "absent keeps the cache path"
                    ),
                    None => panic!("absent keeps the cache path: {error}"),
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
            Err(error) => panic!("cache stores: {error}"),
        }
        std::fs::create_dir_all(dir.path().join("config")).unwrap();
        std::fs::write(dir.path().join("config").join("blobs"), b"not a dir").unwrap();
        match store.persist(&sha) {
            Ok(_) => panic!("blocked pool passes"),
            Err(error) => match &error.context.cache_path {
                Some(found) => assert_eq!(
                    found,
                    &pools.path(&sha),
                    "blocked pool keeps the cache path"
                ),
                None => panic!("blocked pool keeps the cache path: {error}"),
            },
        }
    }
}
