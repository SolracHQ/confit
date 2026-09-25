//! Memory
//!
//! Memory blob pool for tests.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use confit_core::error::{Error, Result};
use confit_core::handles::{BlobHandle, Sha, TrustedHandle};

use super::BlobStore;

/// Copy chunk size for trusted source streaming.
const SOURCE_CHUNK: usize = 8192;

/// Memory blob pool for tests.
///
/// Blobs ride a `stored -> bytes` map behind one lock.
/// Stored bytes hold raw content, so stored and content
/// hashes match here while keying still mirrors the file pool.
#[derive(Debug, Default)]
pub struct MemoryBlobStore {
    blobs: Mutex<HashMap<String, Vec<u8>>>,
}

impl MemoryBlobStore {
    /// Builds an empty memory blob pool.
    pub fn new() -> Self {
        Self {
            blobs: Mutex::new(HashMap::new()),
        }
    }
}

impl BlobStore for MemoryBlobStore {
    fn open(&self, handle: &BlobHandle) -> Result<Box<dyn std::io::Read>> {
        let guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(handle.stored().hex().as_str()) {
            Some(bytes) => {
                if Sha::hash(bytes) != *handle.sha() {
                    return Err(Error::Plan(format!(
                        "blob '{}' fails verification",
                        handle.sha()
                    )));
                }
                Ok(Box::new(std::io::Cursor::new(bytes.clone())) as Box<dyn std::io::Read>)
            }
            None => Err(Error::Plan(format!("missing blob '{}'", handle.sha()))),
        }
    }

    fn put(&self, bytes: &[u8]) -> Result<BlobHandle> {
        let handle = BlobHandle::new(Sha::hash(bytes), Sha::hash(bytes))?;
        let mut guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(handle.stored().hex(), bytes.to_vec());
        Ok(handle)
    }

    fn put_source(&self, source: &dyn TrustedHandle) -> Result<BlobHandle> {
        let bytes = read_source(source.canonical())?;
        let handle = BlobHandle::new(source.sha().clone(), source.sha().clone())?;
        let mut guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.entry(handle.stored().hex()).or_insert(bytes);
        Ok(handle)
    }

    fn put_reader(&self, reader: &mut dyn std::io::Read) -> Result<BlobHandle> {
        let bytes = read_stream(reader)?;
        let handle = BlobHandle::new(Sha::hash(&bytes), Sha::hash(&bytes))?;
        let mut guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.entry(handle.stored().hex()).or_insert(bytes);
        Ok(handle)
    }

    fn has(&self, handle: &BlobHandle) -> bool {
        let guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.contains_key(handle.stored().hex().as_str())
    }

    fn len(&self, handle: &BlobHandle) -> Result<u64> {
        let guard = match self.blobs.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(handle.stored().hex().as_str()) {
            Some(bytes) => Ok(bytes.len() as u64),
            None => Err(Error::Plan(format!("missing blob '{}'", handle.sha()))),
        }
    }

    fn prune(&self) -> Result<usize> {
        Ok(0)
    }
}

/// Raw bytes for one source path.
///
/// # Errors
///
/// Unreadable sources fail as plan errors naming the path.
fn read_source(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .map_err(|error| Error::Plan(format!("read source '{}': {error}", path.display())))?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; SOURCE_CHUNK];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|error| Error::Plan(format!("read source '{}': {error}", path.display())))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(bytes)
}

/// Raw bytes for one byte stream.
///
/// # Errors
///
/// Unreadable streams fail as plan errors.
fn read_stream(reader: &mut dyn std::io::Read) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut chunk = [0u8; SOURCE_CHUNK];
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|error| Error::Plan(format!("read blob stream: {error}")))?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    struct TestSource(PathBuf, Sha);

    impl TrustedHandle for TestSource {
        fn canonical(&self) -> &Path {
            &self.0
        }

        fn sha(&self) -> &Sha {
            &self.1
        }
    }

    fn large_source_bytes() -> Vec<u8> {
        (0..20 * 1024).map(|index| (index % 251) as u8).collect()
    }

    fn write_source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn put_source_roundtrip_streams_large_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryBlobStore::new();
        let raw = large_source_bytes();
        assert!(raw.len() > SOURCE_CHUNK);
        let path = write_source(dir.path(), "input.bin", &raw);
        let handle = store
            .put_source(&TestSource(path, Sha::hash(&raw)))
            .unwrap();
        assert_eq!(handle.sha(), &Sha::hash(&raw));
        assert!(store.has(&handle));
        match store.open(&handle) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("source blob opens: {error}"),
        }
    }

    #[test]
    fn put_source_skips_present_hash() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryBlobStore::new();
        let raw = large_source_bytes();
        let via_put = store.put(&raw).unwrap();
        let path = write_source(dir.path(), "input.bin", &raw);
        let first = store
            .put_source(&TestSource(path, via_put.sha().clone()))
            .unwrap();
        assert_eq!(first, via_put);
        let path = write_source(dir.path(), "input.bin", &raw);
        let second = store
            .put_source(&TestSource(path, via_put.sha().clone()))
            .unwrap();
        assert_eq!(second, via_put);
        match store.open(&via_put) {
            Ok(mut reader) => {
                let mut found = Vec::new();
                reader.read_to_end(&mut found).unwrap();
                assert_eq!(found, raw);
            }
            Err(error) => panic!("seeded blob opens: {error}"),
        }
    }

    #[test]
    fn put_source_missing_names_path() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryBlobStore::new();
        let path = dir.path().join("absent.bin");
        match store.put_source(&TestSource(path.clone(), Sha::hash(b"absent"))) {
            Ok(_) => panic!("missing source passes"),
            Err(error) => assert!(
                error.to_string().contains(&path.display().to_string()),
                "error names the path: {error}"
            ),
        }
    }

    #[test]
    fn put_source_trusts_handle_sha_over_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryBlobStore::new();
        let raw = b"shared bytes".to_vec();
        let sealed = store.put(&raw).unwrap();
        let path = write_source(dir.path(), "a.bin", &raw);
        let again = store
            .put_source(&TestSource(path, sealed.sha().clone()))
            .unwrap();
        assert_eq!(again, sealed);
        assert!(store.has(&sealed));
        let mut found = Vec::new();
        store
            .open(&sealed)
            .unwrap()
            .read_to_end(&mut found)
            .unwrap();
        assert_eq!(found, raw);
    }

    #[test]
    fn put_source_detects_forged_handle_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let store = MemoryBlobStore::new();
        let raw = b"shared bytes".to_vec();
        let forged = store
            .put_source(&TestSource(
                write_source(dir.path(), "a.bin", &raw),
                Sha::hash(b"forged-identity"),
            ))
            .unwrap();
        assert!(store.has(&forged));
        match store.open(&forged) {
            Ok(_) => panic!("forged blob opens"),
            Err(error) => assert!(error.to_string().contains("fails verification")),
        }
    }

    #[test]
    fn put_reader_matches_put_for_identical_content() {
        let store = MemoryBlobStore::new();
        let raw = large_source_bytes();
        assert!(raw.len() > SOURCE_CHUNK);
        let via_put = store.put(&raw).unwrap();
        let mut reader = std::io::Cursor::new(raw.clone());
        let via_reader = store.put_reader(&mut reader).unwrap();
        assert_eq!(via_reader, via_put, "reader path keeps sealed identity");
        let mut found = Vec::new();
        store
            .open(&via_reader)
            .unwrap()
            .read_to_end(&mut found)
            .unwrap();
        assert_eq!(found, raw);
    }
}
