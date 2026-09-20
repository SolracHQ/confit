//! Blobs
//!
//! Shared blob pool plus hydration.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::document::ManifestData;
use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::ids::DocPath;
use crate::plan::{BUNDLE_VERSION, Bundle};
use crate::progress::{Event, ProgressSender};

use super::manifest::Manifest;
use super::slots::{default_state_path, resolve_base_dir, resolve_plans_dir, resolve_previous_dir};

/// Pool folder name under the base folder.
const BLOBS_DIR: &str = "blobs";

/// Gzip level for pooled plus inner bundle blob bytes.
const BLOB_GZIP_LEVEL: u32 = 6;

/// Blob hash length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;

/// Resolves the shared blob pool folder under the base.
///
/// # Returns
///
/// The folder holding gzipped blobs under content hashes.
///
/// # Errors
///
/// Missing OS config folders fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::blobs::resolve_blobs_dir;
///
/// let dir = resolve_blobs_dir();
/// assert!(matches!(dir, Ok(dir) if dir.ends_with("confit/blobs")));
/// ```
pub fn resolve_blobs_dir() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join(BLOBS_DIR))
}

/// Compresses raw blob bytes for pool plus bundle storage.
///
/// # Arguments
///
/// * `bytes` - the raw bytes under compressing.
///
/// # Returns
///
/// The gzip bytes.
///
/// # Errors
///
/// Encoder failures surface as plan errors.
pub(crate) fn gzip_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let mut encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(BLOB_GZIP_LEVEL));
    encoder
        .write_all(bytes)
        .map_err(|error| Error::Plan(format!("compress blob: {error}")))?;
    encoder
        .finish()
        .map_err(|error| Error::Plan(format!("compress blob: {error}")))
}

/// Decompresses pool bytes and verifies them against the hash.
///
/// # Arguments
///
/// * `bytes` - the gzip bytes under reading.
/// * `sha` - the expected SHA-256 hex over raw bytes.
///
/// # Returns
///
/// The verified raw bytes.
///
/// # Errors
///
/// Decoder plus hash mismatch failures surface as bundle
/// errors naming the hash.
pub(crate) fn gunzip_bytes(bytes: &[u8], sha: &str) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?;
    if crate::plan::sha256_hex(&raw) != sha {
        return Err(Error::Plan(format!("blob '{sha}' fails verification")));
    }
    Ok(raw)
}

/// Checks one blob reference holds 64 hex chars.
///
/// # Arguments
///
/// * `sha` - the blob reference under checking.
///
/// # Returns
///
/// Unit for well shaped hashes.
///
/// # Errors
///
/// Malformed references fail as plan errors naming the value.
pub(crate) fn check_blob_id(sha: &str) -> Result<()> {
    if sha.len() == BLOB_ID_LEN && sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Error::Plan(format!(
            "bad blob ref '{sha}': want {BLOB_ID_LEN} hex chars"
        )))
    }
}

/// Collects pooled blob bytes under content hashes in order.
///
/// The bundle map already holds raw bytes under hashes, so
/// collection reads the map straight into sorted order.
///
/// # Arguments
///
/// * `bundle` - the bundle holding blob bytes.
///
/// # Returns
///
/// Content hashes mapping to raw bytes in sorted order.
pub(crate) fn collect_blobs(bundle: &Bundle) -> BTreeMap<String, &[u8]> {
    bundle
        .blobs
        .iter()
        .map(|(sha, bytes)| (sha.clone(), bytes.as_slice()))
        .collect()
}

/// Writes every referenced blob missing from the pool.
///
/// Present hashes skip, so repeated plans share stored
/// bytes. Blobs land gzipped under their content hash.
///
/// # Arguments
///
/// * `bundle` - the live bundle holding binary bytes.
/// * `fs` - the backend under writing.
/// * `progress` - the sink for compression facts, holding `None` for silence.
///
/// # Returns
///
/// Unit once missing blobs land.
///
/// # Errors
///
/// Compression plus write failures surface as plan errors.
pub(crate) fn store_blobs(
    bundle: &Bundle,
    fs: &dyn Filesystem,
    progress: Option<&ProgressSender>,
) -> Result<()> {
    let dir = resolve_blobs_dir()?;
    let mut missing: Vec<(String, Vec<u8>)> = Vec::new();
    for (sha, bytes) in collect_blobs(bundle) {
        let dest = dir.join(&sha);
        if fs.exists(&dest) {
            continue;
        }
        missing.push((sha, bytes.to_vec()));
    }
    let total = missing.len();
    if total > 0
        && let Some(sender) = progress
    {
        let bytes: u64 = missing.iter().map(|(_, raw)| raw.len() as u64).sum();
        let _ = sender.send(Event::CompressStarted {
            blobs: total,
            bytes,
        });
    }
    let indexed: Vec<(usize, String, Vec<u8>)> = missing
        .into_iter()
        .enumerate()
        .map(|(index, (sha, raw))| (index, sha, raw))
        .collect();
    let compressed: Vec<Result<(String, Vec<u8>)>> = indexed
        .par_iter()
        .map(|(index, sha, raw)| {
            let raw_len = raw.len() as u64;
            gzip_bytes(raw).map(|gzipped| {
                if let Some(sender) = progress {
                    let _ = sender.send(Event::BlobCompressed {
                        done: index + 1,
                        total,
                        bytes: raw_len,
                    });
                }
                (sha.clone(), gzipped)
            })
        })
        .collect();
    for entry in compressed {
        let (sha, gzipped) = entry?;
        let dest = dir.join(&sha);
        fs.write(&dest, &gzipped)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
    }
    Ok(())
}

/// Pool blob reader with disk short-circuit plus memory cache.
///
/// Disk destinations matching a blob hash hydrate straight
/// from disk bytes, so steady plans skip pool reads. Pool
/// hits verify hashes and cache per hydration run. Bundle
/// map hits win before both, so archived plans hydrate
/// without pool access.
pub(crate) struct Hydrator<'a> {
    /// Holds the manifest path for error context.
    source: PathBuf,
    /// Holds the pool folder holding gzip blobs.
    pool: PathBuf,
    /// Holds the backend under reading.
    fs: &'a dyn Filesystem,
    /// Holds verified raw bytes per blob hash.
    cache: BTreeMap<String, Vec<u8>>,
}

impl<'a> Hydrator<'a> {
    /// Builds a blob reader for one manifest file.
    pub(crate) fn new(source: &Path, fs: &'a dyn Filesystem) -> Result<Self> {
        Ok(Self {
            source: source.to_path_buf(),
            pool: resolve_blobs_dir()?,
            fs,
            cache: BTreeMap::new(),
        })
    }

    /// Rebuilds the bundle with lazy blob hydration.
    ///
    /// The manifest carries over intact as the only document
    /// language. Every referenced blob resolves through disk
    /// short-circuit plus pool reads into the bundle map.
    pub(crate) fn hydrate(&mut self, stored: &Manifest) -> Result<Bundle> {
        let mut blobs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
        for manifest in &stored.documents {
            let dest = manifest.path.expand();
            let mut hints: BTreeMap<&str, PathBuf> = BTreeMap::new();
            match &manifest.data {
                ManifestData::Opaque { blob, .. } => {
                    hints.insert(blob.as_str(), dest);
                }
                ManifestData::Tree { members } => {
                    for member in members {
                        hints
                            .entry(member.blob.as_str())
                            .or_insert_with(|| dest.join(&member.relative));
                    }
                }
                ManifestData::Structured { .. }
                | ManifestData::Text { .. }
                | ManifestData::Link { .. }
                | ManifestData::Rc(_) => {}
            }
            let path = &manifest.path;
            for sha in manifest.data.blob_refs() {
                if blobs.contains_key(sha) {
                    continue;
                }
                let disk = hints.get(sha).map(|hint| hint.as_path());
                blobs.insert(sha.to_string(), self.blob_bytes(sha, disk, path)?);
            }
        }
        Ok(Bundle {
            manifest: stored.clone(),
            blobs,
        })
    }

    /// Reads verified raw bytes for one blob hash.
    ///
    /// Disk bytes matching the hash win before any pool
    /// read. Pool hits verify plus cache per run. Missing
    /// pool entries fail naming the hash.
    fn blob_bytes(&mut self, sha: &str, disk: Option<&Path>, path: &DocPath) -> Result<Vec<u8>> {
        if sha.len() != BLOB_ID_LEN || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Plan(format!(
                "read state '{}': bad blob ref '{sha}' for '{}'",
                self.source.display(),
                path.as_str()
            )));
        }
        if let Some(dest) = disk
            && let Ok(bytes) = self.fs.read(dest)
            && crate::plan::sha256_hex(&bytes) == sha
        {
            return Ok(bytes);
        }
        if let Some(hit) = self.cache.get(sha) {
            return Ok(hit.clone());
        }
        let dest = self.pool.join(sha);
        let gzipped = match self.fs.read(&dest) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Plan(format!(
                    "read state '{}': missing blob '{sha}' for '{}'",
                    self.source.display(),
                    path.as_str()
                )));
            }
            Err(error) => return Err(Error::from(error)),
        };
        let raw = gunzip_bytes(&gzipped, sha)?;
        self.cache.insert(sha.to_string(), raw.clone());
        Ok(raw)
    }
}

/// Drops pool blobs unreferenced by slot plus history plus named manifests.
///
/// # Arguments
///
/// * `fs` - the backend under pruning.
///
/// # Returns
///
/// The removed blob count.
///
/// # Errors
///
/// Listing plus removal failures surface as plan or io errors.
///
pub fn prune_blobs(fs: &dyn Filesystem) -> Result<usize> {
    let mut keep: BTreeSet<String> = BTreeSet::new();
    if let Ok(slot) = default_state_path() {
        collect_manifest_refs(&slot, fs, &mut keep);
    }
    if let Ok(dir) = resolve_previous_dir() {
        collect_dir_refs(&dir, fs, &mut keep)?;
    }
    if let Ok(dir) = resolve_plans_dir() {
        collect_dir_refs(&dir, fs, &mut keep)?;
    }
    let pool = resolve_blobs_dir()?;
    let entries = match fs.list_dir(&pool) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(Error::from(error)),
    };
    let mut removed = 0;
    for entry in entries {
        let Some(name) = entry
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        if keep.contains(&name) {
            continue;
        }
        fs.remove(&entry).map_err(Error::from)?;
        removed += 1;
    }
    Ok(removed)
}

/// Collects blob refs from every manifest file in one folder.
///
/// Unreadable plus unparsable files skip quietly, matching
/// history listing behavior.
fn collect_dir_refs(dir: &Path, fs: &dyn Filesystem, keep: &mut BTreeSet<String>) -> Result<()> {
    let mut files = match fs.list_dir(dir) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(Error::from(error)),
    };
    files.sort();
    for file in files {
        collect_manifest_refs(&file, fs, keep);
    }
    Ok(())
}

/// Collects blob refs from one manifest file without hydrating.
///
/// Missing plus unparsable plus stale files add no refs.
fn collect_manifest_refs(path: &Path, fs: &dyn Filesystem, keep: &mut BTreeSet<String>) {
    let bytes = match fs.read(path) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let stored: Manifest = match serde_json::from_slice(&bytes) {
        Ok(stored) => stored,
        Err(_) => return,
    };
    if stored.version != BUNDLE_VERSION {
        return;
    }
    for document in &stored.documents {
        keep.extend(document.data.blob_refs().into_iter().map(str::to_string));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::plan::Bundle;
    use crate::store::slots::{
        default_state_path, load_state, resolve_named_slot, resolve_previous_dir, write_manifest,
    };

    pub(crate) fn mixed_plan() -> Bundle {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::ids::DocPath;

        let opaque_blob = crate::plan::sha256_hex(&[0xFF, 0x00, 0x41]);
        let a_blob = crate::plan::sha256_hex(&[1, 2, 3]);
        let b_blob = crate::plan::sha256_hex(&[4, 5, 6]);
        let documents = vec![
            ManifestDocument::new(
                DocPath::new("note"),
                ManifestData::Text {
                    content: "hi".to_string(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("bin"),
                ManifestData::Opaque {
                    blob: opaque_blob.clone(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("fonts"),
                ManifestData::Tree {
                    members: vec![
                        ManifestMember {
                            relative: "a.ttf".into(),
                            blob: a_blob.clone(),
                            mode: 0o644,
                        },
                        ManifestMember {
                            relative: "b.ttf".into(),
                            blob: b_blob.clone(),
                            mode: 0o644,
                        },
                    ],
                },
            ),
        ];
        let mut built = match Bundle::build(documents, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        built.blobs.insert(opaque_blob, vec![0xFF, 0x00, 0x41]);
        built.blobs.insert(a_blob, vec![1, 2, 3]);
        built.blobs.insert(b_blob, vec![4, 5, 6]);
        built
    }

    pub(crate) fn pool_blobs(fs: &crate::fs::MemoryFs) -> Vec<std::path::PathBuf> {
        let dir = match resolve_blobs_dir() {
            Ok(dir) => dir,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        match fs.list_dir(&dir) {
            Ok(mut entries) => {
                entries.sort();
                entries
            }
            Err(error) => panic!("pool lists: {error}"),
        }
    }

    fn gunzip_blob(fs: &crate::fs::MemoryFs, path: &std::path::Path) -> Vec<u8> {
        use std::io::Read as _;

        let gzipped = match fs.read(path) {
            Ok(bytes) => bytes,
            Err(error) => panic!("blob reads: {error}"),
        };
        let mut decoder = flate2::read::GzDecoder::new(&gzipped[..]);
        let mut raw = Vec::new();
        match decoder.read_to_end(&mut raw) {
            Ok(_) => {}
            Err(error) => panic!("blob gunzips: {error}"),
        }
        raw
    }

    #[test]
    fn manifest_pool_roundtrip_through_memory_fs() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_eq!(loaded, built);
        let blobs = pool_blobs(&fs);
        assert_eq!(blobs.len(), 3);
        let mut raws: Vec<Vec<u8>> = blobs.iter().map(|blob| gunzip_blob(&fs, blob)).collect();
        raws.sort();
        assert_eq!(
            raws,
            vec![vec![1, 2, 3], vec![4, 5, 6], vec![0xFF, 0x00, 0x41]]
        );
        let text = match fs.read(dest) {
            Ok(bytes) => bytes,
            Err(error) => panic!("manifest reads: {error}"),
        };
        let text = match String::from_utf8(text) {
            Ok(text) => text,
            Err(error) => panic!("manifest decodes: {error}"),
        };
        assert!(text.contains("\"blob\""));
        assert!(!text.contains("/wBB"));
        assert!(!text.contains("AQID"));
        assert!(!text.contains("BAUG"));
    }

    #[test]
    fn identical_bytes_share_one_pool_blob() {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::ids::DocPath;

        let fs = crate::fs::MemoryFs::new();
        let shared = vec![9, 9, 9];
        let shared_blob = crate::plan::sha256_hex(&shared);
        let documents = vec![
            ManifestDocument::new(
                DocPath::new("first"),
                ManifestData::Opaque {
                    blob: shared_blob.clone(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("second"),
                ManifestData::Opaque {
                    blob: shared_blob.clone(),
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("fonts"),
                ManifestData::Tree {
                    members: vec![ManifestMember {
                        relative: "a.ttf".into(),
                        blob: shared_blob.clone(),
                        mode: 0o644,
                    }],
                },
            ),
        ];
        let mut built = match Bundle::build(documents, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        built.blobs.insert(shared_blob, shared.clone());
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let blobs = pool_blobs(&fs);
        assert_eq!(blobs.len(), 1);
        assert_eq!(gunzip_blob(&fs, &blobs[0]), shared);
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn prune_blobs_drops_only_unreferenced() {
        use crate::document::{ManifestData, ManifestDocument};
        use crate::fs::{Filesystem, MemoryFs};
        use crate::ids::DocPath;

        fn opaque_plan(path: &str, byte: u8) -> Bundle {
            let blob = crate::plan::sha256_hex(&[byte]);
            let mut built = match Bundle::build(
                vec![ManifestDocument::new(
                    DocPath::new(path),
                    ManifestData::Opaque {
                        blob: blob.clone(),
                        mode: None,
                        unmanaged: false,
                    },
                )],
                Vec::new(),
            ) {
                Ok(built) => built,
                Err(error) => panic!("bundle builds: {error}"),
            };
            built.blobs.insert(blob, vec![byte]);
            built
        }

        let fs = MemoryFs::new();
        let slot = match default_state_path() {
            Ok(slot) => slot,
            Err(error) => panic!("slot resolves: {error}"),
        };
        let slot_plan = opaque_plan("slot-bin", 10);
        match write_manifest(&slot_plan, Some(&slot), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("slot writes: {error}"),
        }
        let previous = match resolve_previous_dir() {
            Ok(previous) => previous,
            Err(error) => panic!("history resolves: {error}"),
        };
        let history_plan = opaque_plan("history-bin", 20);
        match write_manifest(&history_plan, Some(&previous.join("1.json")), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("history writes: {error}"),
        }
        let named = match resolve_named_slot("work") {
            Ok(named) => named,
            Err(error) => panic!("named resolves: {error}"),
        };
        let named_plan = opaque_plan("named-bin", 30);
        match write_manifest(&named_plan, Some(&named), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("named writes: {error}"),
        }
        let pool = match resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        let orphan = pool.join("0".repeat(64));
        match fs.write(&orphan, b"orphan") {
            Ok(()) => {}
            Err(error) => panic!("orphan writes: {error}"),
        }
        match prune_blobs(&fs) {
            Ok(removed) => assert_eq!(removed, 1),
            Err(error) => panic!("blobs prune: {error}"),
        }
        assert!(!fs.exists(&orphan));
        for byte in [10, 20, 30] {
            let kept = pool.join(crate::plan::sha256_hex(&[byte]));
            assert!(fs.exists(&kept));
        }
        let loaded = match load_state(Some(&slot), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("slot loads: {error}"),
        };
        assert_eq!(loaded, slot_plan);
    }

    #[test]
    fn lazy_hydration_skips_pool_on_identical_disk() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        match fs.write(std::path::Path::new("bin"), &[0xFF, 0x00, 0x41]) {
            Ok(()) => {}
            Err(error) => panic!("disk writes: {error}"),
        }
        match fs.write(std::path::Path::new("fonts/a.ttf"), &[1, 2, 3]) {
            Ok(()) => {}
            Err(error) => panic!("disk writes: {error}"),
        }
        match fs.write(std::path::Path::new("fonts/b.ttf"), &[4, 5, 6]) {
            Ok(()) => {}
            Err(error) => panic!("disk writes: {error}"),
        }
        for blob in pool_blobs(&fs) {
            match fs.remove(&blob) {
                Ok(()) => {}
                Err(error) => panic!("pool clears: {error}"),
            }
        }
        assert!(pool_blobs(&fs).is_empty());
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn lazy_hydration_loads_pool_on_mismatched_disk() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        match fs.write(std::path::Path::new("bin"), b"stale") {
            Ok(()) => {}
            Err(error) => panic!("disk writes: {error}"),
        }
        match fs.write(std::path::Path::new("fonts/a.ttf"), b"stale") {
            Ok(()) => {}
            Err(error) => panic!("disk writes: {error}"),
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn lazy_hydration_rejects_corrupt_pool_blob() {
        use crate::fs::{Filesystem, MemoryFs};
        use std::io::Write as _;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let sha = crate::plan::sha256_hex(&[0xFF, 0x00, 0x41]);
        let pool = match resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        match encoder.write_all(b"tampered") {
            Ok(()) => {}
            Err(error) => panic!("blob compresses: {error}"),
        }
        let tampered = match encoder.finish() {
            Ok(bytes) => bytes,
            Err(error) => panic!("blob finishes: {error}"),
        };
        match fs.write(&pool.join(&sha), &tampered) {
            Ok(()) => {}
            Err(error) => panic!("blob writes: {error}"),
        }
        match load_state(Some(dest), &fs) {
            Ok(_) => panic!("tampered blob passes"),
            Err(error) => assert!(
                error.to_string().contains(&sha),
                "error names the hash: {error}"
            ),
        }
    }

    #[test]
    fn lazy_hydration_missing_blob_fails_naming_hash() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let sha = crate::plan::sha256_hex(&[0xFF, 0x00, 0x41]);
        let pool = match resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        match fs.remove(&pool.join(&sha)) {
            Ok(()) => {}
            Err(error) => panic!("blob removes: {error}"),
        }
        match load_state(Some(dest), &fs) {
            Ok(_) => panic!("missing blob passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains(&sha), "error names the hash: {text}");
                assert!(text.contains("missing blob"), "error reports loss: {text}");
            }
        }
    }

    #[test]
    fn compress_events_cover_every_blob_once() {
        use crate::fs::MemoryFs;
        use crate::progress::Event;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        let (sender, receiver) = crossbeam_channel::unbounded::<Event>();
        match write_manifest(&built, Some(dest), &fs, Some(&sender)) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        drop(sender);
        let mut started: Vec<(usize, u64)> = Vec::new();
        let mut dones: Vec<usize> = Vec::new();
        let mut totals: Vec<usize> = Vec::new();
        let mut compressed_bytes: u64 = 0;
        for event in receiver.iter() {
            match event {
                Event::CompressStarted { blobs, bytes } => started.push((blobs, bytes)),
                Event::BlobCompressed { done, total, bytes } => {
                    dones.push(done);
                    totals.push(total);
                    compressed_bytes += bytes;
                }
                _ => panic!("unexpected progress event"),
            }
        }
        let total = built.blobs.len();
        let raw_bytes: u64 = built.blobs.values().map(|bytes| bytes.len() as u64).sum();
        match started.as_slice() {
            [(blobs, bytes)] => {
                assert_eq!(*blobs, total);
                assert_eq!(*bytes, raw_bytes);
            }
            _ => panic!("one compress start passes: {started:?}"),
        }
        assert_eq!(dones.len(), total);
        for seen in &totals {
            assert_eq!(*seen, total);
        }
        dones.sort();
        let want: Vec<usize> = (1..=total).collect();
        assert_eq!(dones, want);
        assert_eq!(compressed_bytes, raw_bytes);
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn skipped_pool_blobs_emit_nothing() {
        use crate::fs::MemoryFs;
        use crate::progress::Event;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("first bundle writes: {error}"),
        }
        let (sender, receiver) = crossbeam_channel::unbounded::<Event>();
        match write_manifest(&built, Some(dest), &fs, Some(&sender)) {
            Ok(()) => {}
            Err(error) => panic!("second bundle writes: {error}"),
        }
        drop(sender);
        let events: Vec<Event> = receiver.iter().collect();
        assert!(events.is_empty(), "second run stays silent: {events:?}");
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_eq!(loaded, built);
    }
}
