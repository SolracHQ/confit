//! Blobs
//!
//! Shared blob pool and hydration.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

/// Spill folder name under the process temp dir.
const SPILL_DIR: &str = "confit-spill";

/// Gzip level for pooled and inner bundle blob bytes.
const BLOB_GZIP_LEVEL: u32 = 6;

/// Blob hash length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;

/// One materialized blob file reference.
///
/// The sha covers raw content bytes. The size counts the
/// materialized file bytes. The path names one file holding
/// those bytes, either a spill file, a pool file, or a
/// destination file. Refs never serialize, they die with
/// the process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    /// Holds the SHA-256 hex over raw content bytes.
    pub sha: String,
    /// Holds the materialized file byte count.
    pub size: u64,
    /// Holds one file holding the bytes.
    pub path: PathBuf,
}

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

/// Spills raw bytes to process-local scratch outside the managed filesystem.
///
/// The spill root is `confit-spill` under the process temp
/// dir. File names are SHA-256 hex over content, so parallel
/// writes of equal bytes land on one path. Present files stay
/// untouched, creation races yield to the first writer.
///
/// # Arguments
///
/// * `bytes` - the raw bytes under spilling.
///
/// # Returns
///
/// The spill file path holding the bytes.
pub fn spill_bytes(bytes: &[u8]) -> PathBuf {
    let sha = crate::ids::sha256_hex(bytes);
    let path = std::env::temp_dir().join(SPILL_DIR).join(&sha);
    if !path.exists() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::File::create_new(&path) {
            Ok(mut file) => {
                let _ = file.write_all(bytes);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => {}
        }
    }
    path
}

/// One resolved blob read source.
///
/// Pool files hold gzip bytes under content hashes. Ref files
/// hold raw bytes at their materialized path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlobSource {
    /// Holds the gzipped pool file under its content hash.
    Pool(PathBuf),
    /// Holds the raw materialized file at its ref path.
    File(PathBuf),
}

/// Resolves one blob hash to its pool-first read source.
///
/// The pool file wins while present, else the ref path reads.
/// Pool hits carry gzip bytes, ref files carry raw bytes.
///
/// # Arguments
///
/// * `sha` - the SHA-256 hex over raw content bytes.
/// * `blobs` - the blob refs under content hashes.
/// * `fs` - the backend under stating.
///
/// # Returns
///
/// The pooled or raw read source.
///
/// # Errors
///
/// Dangling hashes fail as plan errors naming the hash.
pub(crate) fn blob_source(
    sha: &str,
    blobs: &BTreeMap<String, BlobRef>,
    fs: &dyn Filesystem,
) -> Result<BlobSource> {
    let raw = blobs
        .get(sha)
        .ok_or_else(|| Error::Plan(format!("missing blob '{sha}'")))?;
    let pool = resolve_blobs_dir()?.join(sha);
    if fs.exists(&pool) {
        Ok(BlobSource::Pool(pool))
    } else {
        Ok(BlobSource::File(raw.path.clone()))
    }
}

/// Reads verified raw bytes for one blob hash.
///
/// Pool sources gunzip through verification, since pool files
/// hold gzip bytes. Ref files read raw. Missing refs,
/// missing files and corrupt pool entries fail naming the hash.
///
/// # Arguments
///
/// * `sha` - the SHA-256 hex over raw content bytes.
/// * `blobs` - the blob refs under content hashes.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The verified raw bytes.
///
/// # Errors
///
/// Missing refs, unreadable files, and hash mismatches
/// fail as plan errors naming the hash.
pub(crate) fn read_blob_bytes(
    sha: &str,
    blobs: &BTreeMap<String, BlobRef>,
    fs: &dyn Filesystem,
) -> Result<Vec<u8>> {
    match blob_source(sha, blobs, fs)? {
        BlobSource::Pool(path) => {
            let gzipped = fs
                .read(&path)
                .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?;
            gunzip_bytes(&gzipped, sha)
        }
        BlobSource::File(path) => fs
            .read(&path)
            .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}"))),
    }
}

/// Compresses raw blob bytes for pool and bundle storage.
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
/// Decoder and hash mismatch failures surface as bundle
/// errors naming the hash.
pub(crate) fn gunzip_bytes(bytes: &[u8], sha: &str) -> Result<Vec<u8>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|error| Error::Plan(format!("read blob '{sha}': {error}")))?;
    if crate::ids::sha256_hex(&raw) != sha {
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

/// Writes every referenced blob missing from the pool.
///
/// Present hashes skip, so repeated plans share stored
/// bytes. Missing refs stream from their materialized file
/// through gzip into the pool file. Refs already pointing at
/// the pool file skip, their bytes sit home already.
///
/// # Arguments
///
/// * `bundle` - the live bundle holding blob refs.
/// * `fs` - the backend under writing.
/// * `progress` - the sink for compression facts, holding `None` for silence.
///
/// # Returns
///
/// Unit once missing blobs land.
///
/// # Errors
///
/// Missing sources, compression, and write failures
/// surface as plan errors.
pub(crate) fn store_blobs(
    bundle: &Bundle,
    fs: &dyn Filesystem,
    progress: Option<&ProgressSender>,
) -> Result<()> {
    let dir = resolve_blobs_dir()?;
    let mut missing: Vec<&BlobRef> = Vec::new();
    for raw in bundle.blobs.values() {
        let dest = dir.join(&raw.sha);
        if fs.exists(&dest) {
            continue;
        }
        if raw.path == dest {
            continue;
        }
        missing.push(raw);
    }
    let total = missing.len();
    if total > 0
        && let Some(sender) = progress
    {
        let bytes: u64 = missing.iter().map(|raw| raw.size).sum();
        let _ = sender.send(Event::CompressStarted {
            blobs: total,
            bytes,
        });
    }
    for (done, raw) in missing.into_iter().enumerate() {
        let index = done + 1;
        let dest = dir.join(&raw.sha);
        let input = std::fs::File::open(&raw.path)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        let output = fs
            .writer(&dest)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        let mut encoder =
            flate2::write::GzEncoder::new(output, flate2::Compression::new(BLOB_GZIP_LEVEL));
        std::io::copy(&mut std::io::BufReader::new(input), &mut encoder)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        let mut output = encoder
            .finish()
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        output
            .flush()
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
        if let Some(sender) = progress {
            let _ = sender.send(Event::BlobCompressed {
                done: index,
                total,
                bytes: raw.size,
            });
        }
    }
    Ok(())
}

/// Pool blob resolver with disk short-circuit and pool metadata.
///
/// Disk destinations matching a blob hash resolve straight
/// to the disk file, hashing in chunks without holding bytes.
/// Pool hits resolve to the pool file through its length
/// alone. No edge reads content bytes, so hydration holds
/// hashes, sizes, and paths only.
pub(crate) struct Hydrator<'a> {
    /// Holds the manifest path for error context.
    source: PathBuf,
    /// Holds the pool folder holding gzip blobs.
    pool: PathBuf,
    /// Holds the backend under reading.
    fs: &'a dyn Filesystem,
}

impl<'a> Hydrator<'a> {
    /// Builds a blob resolver for one manifest file.
    pub(crate) fn new(source: &Path, fs: &'a dyn Filesystem) -> Result<Self> {
        Ok(Self {
            source: source.to_path_buf(),
            pool: resolve_blobs_dir()?,
            fs,
        })
    }

    /// Rebuilds the bundle with metadata-only blob resolution.
    ///
    /// The manifest carries over intact as the only document
    /// language. Every referenced blob resolves through disk
    /// short-circuit and pool metadata into the ref map.
    /// Disk hits hash the destination file, pool hits read the
    /// pool file length, missing hashes fail naming the hash.
    pub(crate) fn hydrate(&mut self, stored: &Manifest) -> Result<Bundle> {
        let mut blobs: BTreeMap<String, BlobRef> = BTreeMap::new();
        for manifest in &stored.documents {
            let dest = manifest.path.expand();
            match &manifest.data {
                ManifestData::Opaque { blob, .. } => {
                    self.insert_ref(blob, dest, &manifest.path, &mut blobs)?;
                }
                ManifestData::Tree { members } => {
                    for member in members {
                        self.insert_ref(
                            &member.blob,
                            dest.join(&member.relative),
                            &manifest.path,
                            &mut blobs,
                        )?;
                    }
                }
                ManifestData::Structured { .. }
                | ManifestData::Text { .. }
                | ManifestData::Link { .. }
                | ManifestData::Rc(_) => {}
            }
        }
        Ok(Bundle {
            manifest: stored.clone(),
            blobs,
        })
    }

    /// Resolves one blob hash to its disk or pool ref.
    ///
    /// Disk files hashing to the sha win before any pool
    /// check. Pool files resolve through length alone. First
    /// resolution wins for shared hashes. Missing pool entries
    /// fail naming the hash.
    fn insert_ref(
        &self,
        sha: &str,
        disk: PathBuf,
        path: &DocPath,
        blobs: &mut BTreeMap<String, BlobRef>,
    ) -> Result<()> {
        if blobs.contains_key(sha) {
            return Ok(());
        }
        if sha.len() != BLOB_ID_LEN || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(Error::Plan(format!(
                "read state '{}': bad blob ref '{sha}' for '{}'",
                self.source.display(),
                path.as_str()
            )));
        }
        if let Ok((digest, len)) = self.fs.hash_file(&disk)
            && digest == sha
        {
            blobs.insert(
                sha.to_string(),
                BlobRef {
                    sha: sha.to_string(),
                    size: len,
                    path: disk,
                },
            );
            return Ok(());
        }
        let pooled = self.pool.join(sha);
        if self.fs.exists(&pooled) {
            let len = self.fs.file_len(&pooled).map_err(Error::from)?;
            blobs.insert(
                sha.to_string(),
                BlobRef {
                    sha: sha.to_string(),
                    size: len,
                    path: pooled,
                },
            );
            return Ok(());
        }
        Err(Error::Plan(format!(
            "read state '{}': missing blob '{sha}' for '{}'",
            self.source.display(),
            path.as_str()
        )))
    }
}

/// Drops pool blobs unreferenced by slot, history, and named manifests.
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
/// Listing and removal failures surface as plan or io errors.
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
/// Unreadable and unparsable files skip quietly, matching
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
/// Missing, unparsable, and stale files add no refs.
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

    pub(crate) fn mixed_plan(fs: &crate::fs::memory::MemoryFs) -> Bundle {
        use crate::document::{ManifestData, ManifestDocument, ManifestMember};
        use crate::fs::Filesystem as _;
        use crate::ids::DocPath;

        let opaque_blob = crate::ids::sha256_hex(&[0xFF, 0x00, 0x41]);
        let a_blob = crate::ids::sha256_hex(&[1, 2, 3]);
        let b_blob = crate::ids::sha256_hex(&[4, 5, 6]);
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
                    size: 3,
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
                            size: 3,
                            mode: 0o644,
                        },
                        ManifestMember {
                            relative: "b.ttf".into(),
                            blob: b_blob.clone(),
                            size: 3,
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
        for raw in [vec![0xFF, 0x00, 0x41], vec![1, 2, 3], vec![4, 5, 6]] {
            let sha = crate::ids::sha256_hex(&raw);
            let path = spill_bytes(&raw);
            if let Err(error) = fs.write(&path, &raw) {
                panic!("spill mirrors: {error}");
            }
            built.blobs.insert(
                sha.clone(),
                BlobRef {
                    sha,
                    size: raw.len() as u64,
                    path,
                },
            );
        }
        built
    }

    /// Builds one spill ref for raw bytes.
    fn spill_ref(raw: &[u8]) -> BlobRef {
        let sha = crate::ids::sha256_hex(raw);
        BlobRef {
            sha: sha.clone(),
            size: raw.len() as u64,
            path: spill_bytes(raw),
        }
    }

    /// Asserts manifest equality and ref sha equality.
    ///
    /// Ref paths and sizes vary by resolution edge (spill,
    /// disk, pool), so equality runs on content identity alone.
    pub(crate) fn assert_same_content(built: &Bundle, loaded: &Bundle) {
        assert_eq!(loaded.manifest, built.manifest);
        let built_shas: BTreeSet<&str> = built.blobs.keys().map(String::as_str).collect();
        let loaded_shas: BTreeSet<&str> = loaded.blobs.keys().map(String::as_str).collect();
        assert_eq!(loaded_shas, built_shas);
        for (sha, raw) in &loaded.blobs {
            assert_eq!(&raw.sha, sha);
        }
    }

    pub(crate) fn pool_blobs(fs: &crate::fs::memory::MemoryFs) -> Vec<std::path::PathBuf> {
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

    fn gunzip_blob(fs: &crate::fs::memory::MemoryFs, path: &std::path::Path) -> Vec<u8> {
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
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("bundle loads: {error}"),
        };
        assert_same_content(&built, &loaded);
        let first = spill_bytes(&[0xFF, 0x00, 0x41]);
        let second = spill_bytes(&[0xFF, 0x00, 0x41]);
        assert_eq!(first, second, "spills land once per content");
        let pool = match resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        for (sha, raw) in &loaded.blobs {
            assert_eq!(raw.path, pool.join(sha), "pool hit carries the pool file");
        }
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

        let fs = crate::fs::memory::MemoryFs::new();
        let shared = vec![9, 9, 9];
        let shared_blob = crate::ids::sha256_hex(&shared);
        let documents = vec![
            ManifestDocument::new(
                DocPath::new("first"),
                ManifestData::Opaque {
                    blob: shared_blob.clone(),
                    size: 3,
                    mode: None,
                    unmanaged: false,
                },
            ),
            ManifestDocument::new(
                DocPath::new("second"),
                ManifestData::Opaque {
                    blob: shared_blob.clone(),
                    size: 3,
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
                        size: 3,
                        mode: 0o644,
                    }],
                },
            ),
        ];
        let mut built = match Bundle::build(documents, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let shared_ref = spill_ref(&shared);
        built.blobs.insert(shared_blob, shared_ref);
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
        assert_same_content(&built, &loaded);
    }

    #[test]
    fn prune_blobs_drops_only_unreferenced() {
        use crate::document::{ManifestData, ManifestDocument};
        use crate::fs::{Filesystem, memory::MemoryFs};
        use crate::ids::DocPath;

        fn opaque_plan(path: &str, byte: u8) -> Bundle {
            let blob = crate::ids::sha256_hex(&[byte]);
            let mut built = match Bundle::build(
                vec![ManifestDocument::new(
                    DocPath::new(path),
                    ManifestData::Opaque {
                        blob: blob.clone(),
                        size: 1,
                        mode: None,
                        unmanaged: false,
                    },
                )],
                Vec::new(),
            ) {
                Ok(built) => built,
                Err(error) => panic!("bundle builds: {error}"),
            };
            built.blobs.insert(blob, spill_ref(&[byte]));
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
            let kept = pool.join(crate::ids::sha256_hex(&[byte]));
            assert!(fs.exists(&kept));
        }
        let loaded = match load_state(Some(&slot), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("slot loads: {error}"),
        };
        assert_same_content(&slot_plan, &loaded);
    }

    #[test]
    fn lazy_hydration_skips_pool_on_identical_disk() {
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
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
        assert_same_content(&built, &loaded);
        let bin_dest = crate::ids::DocPath::new("bin").expand();
        let fonts_dest = crate::ids::DocPath::new("fonts").expand();
        for (raw_bytes, relative) in [
            (vec![0xFF, 0x00, 0x41], None),
            (vec![1, 2, 3], Some("a.ttf")),
            (vec![4, 5, 6], Some("b.ttf")),
        ] {
            let sha = crate::ids::sha256_hex(&raw_bytes);
            let want = match relative {
                Some(name) => fonts_dest.join(name),
                None => bin_dest.clone(),
            };
            match loaded.blobs.get(&sha) {
                Some(raw) => {
                    assert_eq!(raw.path, want, "disk hit carries the dest file");
                    assert_eq!(raw.size, raw_bytes.len() as u64);
                }
                None => panic!("disk ref resolves"),
            }
        }
    }

    #[test]
    fn lazy_hydration_loads_pool_on_mismatched_disk() {
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
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
        assert_same_content(&built, &loaded);
        let pool = match resolve_blobs_dir() {
            Ok(pool) => pool,
            Err(error) => panic!("blobs resolve: {error}"),
        };
        for (sha, raw) in &loaded.blobs {
            assert_eq!(raw.path, pool.join(sha), "pool hit carries the pool file");
            match fs.file_len(&raw.path) {
                Ok(len) => assert_eq!(raw.size, len),
                Err(error) => panic!("pool lengths: {error}"),
            }
        }
    }

    #[test]
    fn lazy_hydration_rejects_corrupt_pool_blob() {
        use crate::fs::{Filesystem, memory::MemoryFs};
        use std::io::Write as _;

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let sha = crate::ids::sha256_hex(&[0xFF, 0x00, 0x41]);
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
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("refs resolve without reads: {error}"),
        };
        match read_blob_bytes(&sha, &loaded.blobs, &fs) {
            Ok(_) => panic!("tampered blob passes"),
            Err(error) => assert!(
                error.to_string().contains(&sha),
                "error names the hash: {error}"
            ),
        }
    }

    #[test]
    fn lazy_hydration_missing_blob_fails_naming_hash() {
        use crate::fs::{Filesystem, memory::MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
        let dest = std::path::Path::new("bundle.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let sha = crate::ids::sha256_hex(&[0xFF, 0x00, 0x41]);
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
        use crate::fs::memory::MemoryFs;
        use crate::progress::Event;

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
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
        let raw_bytes: u64 = built.blobs.values().map(|raw| raw.size).sum();
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
        assert_same_content(&built, &loaded);
    }

    #[test]
    fn skipped_pool_blobs_emit_nothing() {
        use crate::fs::memory::MemoryFs;
        use crate::progress::Event;

        let fs = MemoryFs::new();
        let built = mixed_plan(&fs);
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
        assert_same_content(&built, &loaded);
    }
}
