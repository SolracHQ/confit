//! Store
//!
//! Manifests plus blob pool plus document writes plus history rotation.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::document::{Document, DocumentData, ManifestData, ManifestDocument, TreeMember};
use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::hook::Hook;
use crate::ids::DocPath;
use crate::plan::{PLAN_VERSION, Plan};

/// One stored plan entry for the apply-past listing.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::PreviousEntry;
///
/// let entry = PreviousEntry { index: 1, created_at: String::new() };
/// assert!(matches!(entry.index, 1));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousEntry {
    /// Holds the listing position used as the apply `%N` pick.
    pub index: usize,
    /// Holds the stored plan creation timestamp.
    pub created_at: String,
}

/// Persisted plan holding metadata plus blob references.
///
/// Binary bytes live gzipped in the shared pool under
/// content hashes. Text, structured, rc, plus link payloads
/// stay inline. Bundles carry this shape as `manifest.json`.
///
/// # Examples
///
/// ```rust
/// use confit_core::plan::Plan;
/// use confit_core::store::Manifest;
///
/// let stored = Manifest::of(&Plan::empty());
/// assert!(matches!(stored.documents.len(), 0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Holds the plan format version.
    pub version: u32,
    /// Holds persisted documents in path order.
    pub documents: Vec<ManifestDocument>,
    /// Holds the RFC3339 creation timestamp.
    pub created_at: String,
    /// Holds merged hooks in first-seen order.
    #[serde(default)]
    pub hooks: Vec<Hook>,
}

impl Manifest {
    /// Builds the persisted plan holding blob references.
    ///
    /// # Arguments
    ///
    /// * `plan` - the live plan holding binary bytes.
    ///
    /// # Returns
    ///
    /// The manifest for plan files plus bundles.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use confit_core::plan::Plan;
    /// use confit_core::store::Manifest;
    ///
    /// let stored = Manifest::of(&Plan::empty());
    /// assert!(matches!(stored.version, v if v == confit_core::plan::PLAN_VERSION));
    /// ```
    pub fn of(plan: &Plan) -> Self {
        Self {
            version: plan.version,
            documents: plan
                .documents
                .iter()
                .map(|document| document.manifest_document())
                .collect(),
            created_at: plan.created_at.clone(),
            hooks: plan.hooks.clone(),
        }
    }
}

/// Pool folder name under the base folder.
const BLOBS_DIR: &str = "blobs";
/// Gzip level for pooled plus bundled blobs.
const BLOB_GZIP_LEVEL: u32 = 9;
/// Blob hash length in lowercase hex chars.
const BLOB_ID_LEN: usize = 64;
/// Bundle manifest file name inside the archive.
const BUNDLE_MANIFEST: &str = "manifest.json";
/// Bundle blob folder prefix inside the archive.
const BUNDLE_BLOBS_PREFIX: &str = "blobs/";

/// Loads previous plan, treating missing files as empty.
///
/// The manifest parses in two steps. A version probe runs
/// first. Missing or invalid versions fail. Mismatched
/// versions fail as unsupported. Hashes persist in the file
/// and read trusted, so loads skip rendering. Binary bytes
/// hydrate lazily: disk bytes matching a blob hash win
/// before any pool read, so steady plans skip pool bytes.
/// Missing pool blobs fail naming the hash.
///
/// # Arguments
///
/// * `path` - the state file, holding `None` for empty previous.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The parsed plan, else empty for missing inputs.
///
/// # Errors
///
/// Unreadable present files plus bad JSON plus version
/// mismatch plus missing blobs fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::PLAN_VERSION;
/// use confit_core::store::load_state;
///
/// let outcome = load_state(None, &MemoryFs::new());
/// assert!(matches!(outcome, Ok(plan) if plan.documents.is_empty() && plan.version == PLAN_VERSION));
/// ```
pub fn load_state(path: Option<&Path>, fs: &dyn Filesystem) -> Result<Plan> {
    let Some(file) = path else {
        return Ok(Plan::empty());
    };
    let bytes = match fs.read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Plan::empty());
        }
        Err(error) => return Err(Error::from(error)),
    };
    let read_start = std::time::Instant::now();
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", file.display())))?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(version) if version == u64::from(PLAN_VERSION) => {}
        Some(version) => {
            return Err(Error::Plan(format!(
                "state version {version} reads unsupported, want {PLAN_VERSION}"
            )));
        }
        None => {
            return Err(Error::Plan(format!(
                "read state '{}': missing plan version",
                file.display()
            )));
        }
    }
    let stored: Manifest = serde_json::from_value(value)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", file.display())))?;
    let plan = Hydrator::new(file, fs)?.hydrate(&stored)?;
    log::debug!(
        "read took {}ms for {}",
        read_start.elapsed().as_millis(),
        file.display()
    );
    Ok(plan)
}

/// Serializes one plan as a pretty manifest.
///
/// Binary bytes leave the file as blob references into the
/// shared pool. Output bytes match pretty serde exactly.
///
/// # Arguments
///
/// * `plan` - the plan under serializing.
///
/// # Returns
///
/// The pretty manifest JSON.
///
/// # Errors
///
/// Document serialization failures surface as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::plan::Plan;
/// use confit_core::store::plan_json;
///
/// let plan = Plan::empty();
/// assert!(matches!(plan_json(&plan), Ok(text) if text.contains("documents")));
/// ```
pub fn plan_json(plan: &Plan) -> Result<String> {
    let stored = Manifest::of(plan);
    serde_json::to_string_pretty(&stored)
        .map_err(|error| Error::Plan(format!("render plan: {error}")))
}

/// Writes the manifest payload to a file or stdout.
///
/// Manifest writes store missing pool blobs first, so the
/// file stays resolvable after the write. Both destinations
/// take the pretty manifest form.
///
/// # Arguments
///
/// * `plan` - the versioned desired state.
/// * `out` - the destination, holding `None` for stdout.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once the payload lands.
///
/// # Errors
///
/// Serializer plus io failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::{PLAN_VERSION, Plan};
/// use confit_core::store::write_plan;
///
/// let plan = Plan { version: PLAN_VERSION, documents: Vec::new(), created_at: String::new(), hooks: Vec::new() };
/// assert!(matches!(write_plan(&plan, None, &MemoryFs::new()), Ok(())));
/// ```
pub fn write_plan(plan: &Plan, out: Option<&Path>, fs: &dyn Filesystem) -> Result<()> {
    store_blobs(plan, fs)?;
    let text = plan_json(plan)?;
    match out {
        Some(dest) => fs
            .write(dest, text.as_bytes())
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display()))),
        None => {
            println!("{text}");
            Ok(())
        }
    }
}

/// Writes every document to its expanded path.
///
/// Text plus structured plus rc render through core. Opaque
/// writes raw bytes. Links land as symlinks. Documents carrying
/// a mode set permission bits after their bytes land. Parents
/// build on demand through the backend seam.
///
/// # Arguments
///
/// * `documents` - the desired documents under writing.
/// * `fs` - the backend under writing.
/// * `on_written` - the per-document callback, holding `None` for silence.
///
/// # Returns
///
/// Unit once every document lands.
///
/// # Errors
///
/// Render plus io failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::store::write_documents;
///
/// let fs = MemoryFs::new();
/// let documents = vec![Document::new(
///     DocPath::new("note"),
///     DocumentData::Text { content: "hi".into(), mode: None },
/// )];
/// assert!(matches!(write_documents(&documents, &fs, None), Ok(())));
/// assert!(fs.exists(std::path::Path::new("note")));
/// ```
pub fn write_documents(
    documents: &[Document],
    fs: &dyn Filesystem,
    on_written: Option<&dyn Fn(&DocPath)>,
) -> Result<()> {
    for document in documents {
        let expanded = document.path.expand();
        let outcome = match &document.data {
            DocumentData::Link { target } => fs.symlink(&expanded, Path::new(target)),
            DocumentData::Tree { members } => write_tree_members(&expanded, members, fs),
            _ => {
                let bytes = document.bytes()?;
                fs.write(&expanded, &bytes)
            }
        };
        if let Err(error) = outcome {
            return Err(Error::Plan(format!(
                "cannot write '{}': {error}",
                expanded.display()
            )));
        }
        if let Some(mode) = document.mode()
            && let Err(error) = fs.set_mode(&expanded, mode)
        {
            return Err(Error::Plan(format!(
                "cannot set mode '{}': {error}",
                expanded.display()
            )));
        }
        if let Some(notify) = on_written {
            notify(&document.path);
        }
    }
    Ok(())
}

/// Writes one tree destination member by member.
///
/// Only the members land, never the destination folder
/// itself. Parent folders create as needed, modes land
/// per member from the manifest.
///
/// # Arguments
///
/// * `dest` - the expanded destination folder.
/// * `members` - the desired members under writing.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once every member lands.
///
/// # Errors
///
/// Write plus mode failures surface as io errors carrying
/// the member path.
fn write_tree_members(
    dest: &std::path::Path,
    members: &[crate::document::TreeMember],
    fs: &dyn Filesystem,
) -> std::io::Result<()> {
    for member in members {
        let path = dest.join(&member.rel);
        fs.write(&path, &member.content).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("cannot write '{}': {error}", path.display()),
            )
        })?;
        fs.set_mode(&path, member.mode).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("cannot set mode '{}': {error}", path.display()),
            )
        })?;
    }
    Ok(())
}

/// Removes recorded paths absent from desired documents.
///
/// Only state-recorded paths delete, never anything else.
/// Tree destinations never delete as paths, members
/// reconcile through `remove_tree_members` instead.
/// Already-absent paths stay quiet, matching desired state.
///
/// # Arguments
///
/// * `recorded` - the last recorded documents.
/// * `desired` - the desired documents keeping their paths.
/// * `fs` - the backend under removal.
///
/// # Returns
///
/// The removed path count.
///
/// # Errors
///
/// Removal failures surface as io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::fs::MemoryFs;
/// use confit_core::ids::DocPath;
/// use confit_core::store::remove_orphans;
///
/// let fs = MemoryFs::new();
/// let recorded = vec![Document::new(
///     DocPath::new("gone"),
///     DocumentData::Text { content: "hi".into(), mode: None },
/// )];
/// assert!(matches!(remove_orphans(&recorded, &[], &fs), Ok(0)));
/// ```
pub fn remove_orphans(
    recorded: &[Document],
    desired: &[Document],
    fs: &dyn Filesystem,
) -> Result<usize> {
    let mut removed = 0;
    for old in recorded {
        if old.data.tree_members().is_some() {
            continue;
        }
        let kept = desired.iter().any(|document| document.path == old.path);
        if kept {
            continue;
        }
        let expanded = old.path.expand();
        if !fs.exists(&expanded) {
            continue;
        }
        fs.remove(&expanded).map_err(Error::from)?;
        removed += 1;
    }
    Ok(removed)
}

/// Removes dropped tree members between recorded and desired plans.
///
/// The removal set holds recorded manifest members absent
/// from the desired manifest at the same destination, so
/// files the tree dropped delete while hand-placed files
/// stay untouched. Whole dropped trees remove every
/// recorded member. Destinations never delete.
///
/// # Arguments
///
/// * `recorded` - the last recorded documents.
/// * `desired` - the desired documents holding new manifests.
/// * `fs` - the backend under removal.
///
/// # Returns
///
/// The removed member count.
///
/// # Errors
///
/// Removal failures surface as io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::document::{Document, DocumentData, TreeMember};
/// use confit_core::fs::MemoryFs;
/// use confit_core::ids::DocPath;
/// use confit_core::store::remove_tree_members;
///
/// let fs = MemoryFs::new();
/// let recorded = vec![Document::new(
///     DocPath::new("fonts"),
///     DocumentData::Tree { members: vec![TreeMember { rel: "gone.ttf".into(), content: vec![1], mode: 0o644 }] },
/// )];
/// assert!(matches!(remove_tree_members(&recorded, &[], &fs), Ok(0)));
/// ```
pub fn remove_tree_members(
    recorded: &[Document],
    desired: &[Document],
    fs: &dyn Filesystem,
) -> Result<usize> {
    let mut removed = 0;
    for old in recorded {
        let Some(old_members) = old.data.tree_members() else {
            continue;
        };
        let new_rels: std::collections::BTreeSet<&str> = desired
            .iter()
            .filter(|document| document.path == old.path)
            .filter_map(|document| document.data.tree_members())
            .flat_map(|members| members.iter().map(|member| member.rel.as_str()))
            .collect();
        let dest = old.path.expand();
        for member in old_members {
            if new_rels.contains(member.rel.as_str()) {
                continue;
            }
            let path = dest.join(&member.rel);
            if !fs.exists(&path) {
                continue;
            }
            fs.remove(&path).map_err(Error::from)?;
            removed += 1;
        }
    }
    Ok(removed)
}

/// Stored plans kept before rotation drops the oldest.
const PREVIOUS_KEPT: usize = 5;
/// Lists stored plans newest first with apply picks.
///
/// # Arguments
///
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Stored entries with timestamp per index.
///
/// # Errors
///
/// Folder resolution failures surface as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::store::list_previous;
///
/// let entries = list_previous(&MemoryFs::new());
/// assert!(matches!(entries, Ok(entries) if entries.is_empty()));
/// ```
pub fn list_previous(fs: &dyn Filesystem) -> Result<Vec<PreviousEntry>> {
    let dir = resolve_previous_dir()?;
    Ok(stored_entries(&dir, fs)?
        .into_iter()
        .enumerate()
        .map(|(position, (_, stored))| PreviousEntry {
            index: position + 1,
            created_at: stored.created_at,
        })
        .collect())
}

/// Reads stored plans newest first with their file paths.
///
/// Stamp names stay oldest-first on disk while presentation
/// reverses, so `%1` names the just-previous entry.
/// Manifests hydrate with the disk short-circuit, so steady
/// entries skip pool reads. Unreadable files plus bad JSON
/// plus stale versions plus unresolvable blobs skip quietly.
/// Missing folders read as empty.
///
/// # Arguments
///
/// * `dir` - the history folder under reading.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Stored plans with file paths in sorted order.
///
/// # Errors
///
/// Folder listing failures beyond missing folders surface
/// as io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::store::stored_entries;
/// use std::path::Path;
///
/// let entries = stored_entries(Path::new("previous"), &MemoryFs::new());
/// assert!(matches!(entries, Ok(entries) if entries.is_empty()));
/// ```
pub fn stored_entries(dir: &Path, fs: &dyn Filesystem) -> Result<Vec<(PathBuf, Plan)>> {
    let mut files = match fs.list_dir(dir) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::from(error)),
    };
    files.sort();
    files.reverse();
    let mut out = Vec::new();
    for file in files {
        let bytes = match fs.read(&file) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let stored: Manifest = match serde_json::from_slice(&bytes) {
            Ok(stored) => stored,
            Err(_) => continue,
        };
        if stored.version != PLAN_VERSION {
            continue;
        }
        let mut hydrator = match Hydrator::new(&file, fs) {
            Ok(hydrator) => hydrator,
            Err(_) => continue,
        };
        let Ok(plan) = hydrator.hydrate(&stored) else {
            continue;
        };
        out.push((file, plan));
    }
    Ok(out)
}

/// Reads wall-clock nanos for sortable archive file names.
fn system_nanos() -> Result<u128> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_nanos())
        .map_err(|error| {
            Error::Plan(format!(
                "previous states: clock reads before epoch: {error}"
            ))
        })
}

/// Lists stored files oldest first.
fn history_files(dir: &Path, fs: &dyn Filesystem) -> Result<Vec<PathBuf>> {
    let mut files = match fs.list_dir(dir) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::from(error)),
    };
    files.sort();
    Ok(files)
}

/// Drops stored plans past the kept count, oldest first.
fn rotate_previous(dir: &Path, fs: &dyn Filesystem) -> Result<()> {
    let files = history_files(dir, fs)?;
    if files.len() > PREVIOUS_KEPT {
        for stale in files.iter().take(files.len() - PREVIOUS_KEPT) {
            fs.remove(stale).map_err(Error::from)?;
        }
    }
    Ok(())
}

/// Resolves the fixed base folder under OS config.
///
/// # Returns
///
/// The folder holding the slot plus history.
///
/// # Errors
///
/// Missing OS config folders fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::resolve_base_dir;
///
/// let dir = resolve_base_dir();
/// assert!(matches!(dir, Ok(dir) if dir.ends_with("confit")));
/// ```
pub fn resolve_base_dir() -> Result<PathBuf> {
    match dirs::config_dir() {
        Some(dir) => Ok(dir.join("confit")),
        None => Err(Error::Plan(
            "previous states: cannot resolve OS config folder".to_string(),
        )),
    }
}

/// Resolves the fixed history folder under the base.
///
/// # Returns
///
/// The folder holding stored plans.
///
/// # Errors
///
/// Missing OS config folders fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::resolve_previous_dir;
///
/// let dir = resolve_previous_dir();
/// assert!(matches!(dir, Ok(dir) if dir.ends_with("confit/previous")));
/// ```
pub fn resolve_previous_dir() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("previous"))
}

/// Resolves the named plans folder under the base.
///
/// # Returns
///
/// The folder holding named plans.
///
/// # Errors
///
/// Missing OS config folders fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::resolve_plans_dir;
///
/// let dir = resolve_plans_dir();
/// assert!(matches!(dir, Ok(dir) if dir.ends_with("confit/plans")));
/// ```
pub fn resolve_plans_dir() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("plans"))
}

/// Resolves one named plan file under the plans folder.
///
/// Names hold one file stem with no separators. Empty names,
/// separator carriers, plus dot segments fail as plan errors.
///
/// # Arguments
///
/// * `name` - the plan name without the `@` sigil.
///
/// # Returns
///
/// The `{base}/plans/{name}.json` path.
///
/// # Errors
///
/// Empty names plus separator carriers plus dot segments
/// fail as plan errors. Folder resolution failures surface
/// as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::resolve_named_plan;
///
/// let path = resolve_named_plan("work");
/// assert!(matches!(path, Ok(path) if path.ends_with("confit/plans/work.json")));
/// ```
pub fn resolve_named_plan(name: &str) -> Result<PathBuf> {
    if name.is_empty() {
        return Err(Error::Plan("plan name reads empty".to_string()));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(Error::Plan(format!("plan name '{name}' holds separators")));
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(Error::Plan(format!("plan name '{name}' reads unsupported")));
    }
    if std::path::Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Error::Plan(format!("plan name '{name}' reads unsupported")));
    }
    Ok(resolve_plans_dir()?.join(format!("{name}.json")))
}

/// Builds the fixed live state slot.
///
/// The slot holds the last applied plan. History files
/// live beside it under `previous` with stamp names.
///
/// # Returns
///
/// The slot path, missing while never applied.
///
/// # Errors
///
/// Folder resolution failures surface as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::default_state_path;
///
/// let slot = default_state_path();
/// assert!(matches!(slot, Ok(slot) if slot.ends_with("confit/state.json")));
/// ```
pub fn default_state_path() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("state.json"))
}

/// Stores one applied plan, rotating past the kept count.
///
/// Stamp names sort oldest first. Colliding stamps bump up
/// by one until the name reads fresh. Referenced blobs land
/// in the pool first, so the manifest stays resolvable.
///
/// # Arguments
///
/// * `plan` - the applied plan under storing.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// The stored plan path backing apply of the past.
///
/// # Errors
///
/// Clock plus write failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Plan;
/// use confit_core::store::archive_previous;
///
/// let outcome = archive_previous(&Plan::empty(), &MemoryFs::new());
/// assert!(matches!(outcome, Ok(_)));
/// ```
pub fn archive_previous(plan: &Plan, fs: &dyn Filesystem) -> Result<PathBuf> {
    store_blobs(plan, fs)?;
    let dir = resolve_previous_dir()?;
    let mut stamp = system_nanos()?;
    let mut dest = dir.join(format!("{stamp}.json"));
    while fs.exists(&dest) {
        stamp += 1;
        dest = dir.join(format!("{stamp}.json"));
    }
    let text = plan_json(plan)?;
    fs.write(&dest, text.as_bytes()).map_err(Error::from)?;
    rotate_previous(&dir, fs)?;
    Ok(dest)
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
/// use confit_core::store::resolve_blobs_dir;
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
fn gzip_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
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
/// Decoder plus hash mismatch failures surface as plan
/// errors naming the hash.
fn gunzip_bytes(bytes: &[u8], sha: &str) -> Result<Vec<u8>> {
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
fn check_blob_id(sha: &str) -> Result<()> {
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
/// Opaque documents contribute raw file bytes. Tree
/// documents contribute raw member bytes. Equal bytes share
/// one entry under one hash.
///
/// # Arguments
///
/// * `plan` - the live plan holding binary bytes.
///
/// # Returns
///
/// Content hashes mapping to raw bytes in sorted order.
fn collect_blobs(plan: &Plan) -> BTreeMap<String, &[u8]> {
    let mut out: BTreeMap<String, &[u8]> = BTreeMap::new();
    for document in &plan.documents {
        match &document.data {
            DocumentData::Opaque { content, .. } => {
                out.entry(crate::plan::sha256_hex(content))
                    .or_insert(content);
            }
            DocumentData::Tree { members } => {
                for member in members {
                    out.entry(crate::plan::sha256_hex(&member.content))
                        .or_insert(&member.content);
                }
            }
            DocumentData::Structured { .. }
            | DocumentData::Text { .. }
            | DocumentData::Link { .. }
            | DocumentData::Rc(_) => {}
        }
    }
    out
}

/// Writes every referenced blob missing from the pool.
///
/// Present hashes skip, so repeated plans share stored
/// bytes. Blobs land gzipped under their content hash.
///
/// # Arguments
///
/// * `plan` - the live plan holding binary bytes.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once missing blobs land.
///
/// # Errors
///
/// Compression plus write failures surface as plan errors.
fn store_blobs(plan: &Plan, fs: &dyn Filesystem) -> Result<()> {
    let dir = resolve_blobs_dir()?;
    for (sha, bytes) in collect_blobs(plan) {
        let dest = dir.join(&sha);
        if fs.exists(&dest) {
            continue;
        }
        let gzipped = gzip_bytes(bytes)?;
        fs.write(&dest, &gzipped)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))?;
    }
    Ok(())
}

/// Rebuilds one live payload through a blob reader.
///
/// # Arguments
///
/// * `data` - the persisted payload under hydrating.
/// * `load` - reads verified raw bytes for one blob hash.
///
/// # Returns
///
/// The live payload holding binary bytes.
///
/// # Errors
///
/// Reader failures surface through the reader.
fn hydrate_data(
    data: &ManifestData,
    load: &mut dyn FnMut(&str) -> Result<Vec<u8>>,
) -> Result<DocumentData> {
    match data {
        ManifestData::Structured { format, data } => Ok(DocumentData::Structured {
            format: *format,
            data: data.clone(),
        }),
        ManifestData::Text { content, mode } => Ok(DocumentData::Text {
            content: content.clone(),
            mode: *mode,
        }),
        ManifestData::Link { target } => Ok(DocumentData::Link {
            target: target.clone(),
        }),
        ManifestData::Rc(data) => Ok(DocumentData::Rc(data.clone())),
        ManifestData::Opaque { blob, mode } => Ok(DocumentData::Opaque {
            content: load(blob)?,
            mode: *mode,
        }),
        ManifestData::Tree { members } => {
            let mut live = Vec::with_capacity(members.len());
            for member in members {
                live.push(TreeMember {
                    rel: member.rel.clone(),
                    content: load(&member.blob)?,
                    mode: member.mode,
                });
            }
            Ok(DocumentData::Tree { members: live })
        }
    }
}

/// Pool blob reader with disk short-circuit plus memory cache.
///
/// Disk destinations matching a blob hash hydrate straight
/// from disk bytes, so steady plans skip pool reads. Pool
/// hits verify hashes and cache per hydration run.
struct Hydrator<'a> {
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
    fn new(source: &Path, fs: &'a dyn Filesystem) -> Result<Self> {
        Ok(Self {
            source: source.to_path_buf(),
            pool: resolve_blobs_dir()?,
            fs,
            cache: BTreeMap::new(),
        })
    }

    /// Rebuilds the live plan with lazy blob hydration.
    fn hydrate(&mut self, stored: &Manifest) -> Result<Plan> {
        let mut documents = Vec::with_capacity(stored.documents.len());
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
                            .or_insert_with(|| dest.join(&member.rel));
                    }
                }
                ManifestData::Structured { .. }
                | ManifestData::Text { .. }
                | ManifestData::Link { .. }
                | ManifestData::Rc(_) => {}
            }
            let path = &manifest.path;
            let mut load = |sha: &str| {
                let disk = hints.get(sha).map(|hint| hint.as_path());
                self.blob_bytes(sha, disk, path)
            };
            let data = hydrate_data(&manifest.data, &mut load)?;
            documents.push(Document {
                path: manifest.path.clone(),
                data,
                data_hash: manifest.data_hash.clone(),
            });
        }
        Ok(Plan {
            version: stored.version,
            documents,
            created_at: stored.created_at.clone(),
            hooks: stored.hooks.clone(),
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
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::store::prune_blobs;
///
/// assert!(matches!(prune_blobs(&MemoryFs::new()), Ok(0)));
/// ```
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
    if stored.version != PLAN_VERSION {
        return;
    }
    for document in &stored.documents {
        keep.extend(document.data.blob_refs().into_iter().map(str::to_string));
    }
}

/// Writes one portable bundle holding the manifest plus referenced blobs.
///
/// The tar.gz archive holds `manifest.json` first, then one
/// `blobs/<sha>` gzip entry per referenced blob in sorted
/// order with no duplicates.
///
/// # Arguments
///
/// * `plan` - the live plan under exporting.
/// * `dest` - the bundle file under writing.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// Unit once the bundle lands.
///
/// # Errors
///
/// Compression, archive, plus write failures surface as
/// plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Plan;
/// use confit_core::store::write_bundle;
///
/// let outcome = write_bundle(&Plan::empty(), std::path::Path::new("bundle.tgz"), &MemoryFs::new());
/// assert!(matches!(outcome, Ok(())));
/// ```
pub fn write_bundle(plan: &Plan, dest: &Path, fs: &dyn Filesystem) -> Result<()> {
    let stored = Manifest::of(plan);
    let manifest = serde_json::to_vec_pretty(&stored)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    let encoder =
        flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(BLOB_GZIP_LEVEL));
    let mut builder = tar::Builder::new(encoder);
    append_bundle_entry(&mut builder, BUNDLE_MANIFEST, &manifest, dest)?;
    for (sha, bytes) in collect_blobs(plan) {
        let gzipped = gzip_bytes(bytes)?;
        append_bundle_entry(
            &mut builder,
            &format!("{BUNDLE_BLOBS_PREFIX}{sha}"),
            &gzipped,
            dest,
        )?;
    }
    let encoder = builder
        .into_inner()
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    let archive = encoder
        .finish()
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))?;
    fs.write(dest, &archive)
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", dest.display())))
}

/// Appends one file entry to a bundle archive.
fn append_bundle_entry(
    builder: &mut tar::Builder<flate2::write::GzEncoder<Vec<u8>>>,
    name: &str,
    bytes: &[u8],
    dest: &Path,
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, name, bytes)
        .map_err(|error| Error::Plan(format!("render bundle '{}': {error}", dest.display())))
}

/// Reads one portable bundle into a live plan.
///
/// Blob entries verify against their names before hydrating.
/// Missing blobs fail naming the hash.
///
/// # Arguments
///
/// * `path` - the bundle file under reading.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes.
///
/// # Errors
///
/// Unreadable files plus bad archives plus version mismatch
/// plus missing blobs fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Plan;
/// use confit_core::store::{read_bundle, write_bundle};
///
/// let fs = MemoryFs::new();
/// let dest = std::path::Path::new("bundle.tgz");
/// assert!(matches!(write_bundle(&Plan::empty(), dest, &fs), Ok(())));
/// assert!(matches!(read_bundle(dest, &fs), Ok(plan) if plan.documents.is_empty()));
/// ```
pub fn read_bundle(path: &Path, fs: &dyn Filesystem) -> Result<Plan> {
    let bytes = fs
        .read(path)
        .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
    let decoder = flate2::read::GzDecoder::new(&bytes[..]);
    let mut archive = tar::Archive::new(decoder);
    let mut manifest: Option<Manifest> = None;
    let mut blobs: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let entries = archive
        .entries()
        .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
    for entry in entries {
        let mut entry = entry
            .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?;
        let entry_path = entry
            .path()
            .map_err(|error| Error::Plan(format!("read bundle '{}': {error}", path.display())))?
            .into_owned();
        if entry_path == Path::new(BUNDLE_MANIFEST) {
            if manifest.is_some() {
                return Err(Error::Plan(format!(
                    "read bundle '{}': duplicate manifest",
                    path.display()
                )));
            }
            let mut raw = Vec::new();
            entry.read_to_end(&mut raw).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            let stored: Manifest = serde_json::from_slice(&raw).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            if stored.version != PLAN_VERSION {
                return Err(Error::Plan(format!(
                    "bundle version {} reads unsupported, want {PLAN_VERSION}",
                    stored.version
                )));
            }
            manifest = Some(stored);
        } else if let Ok(rel) = entry_path.strip_prefix(BUNDLE_BLOBS_PREFIX) {
            let sha = match rel.to_str() {
                Some(sha) => sha.to_string(),
                None => {
                    return Err(Error::Plan(format!(
                        "read bundle '{}': bad blob entry",
                        path.display()
                    )));
                }
            };
            if check_blob_id(&sha).is_err() {
                return Err(Error::Plan(format!(
                    "read bundle '{}': bad blob entry '{sha}'",
                    path.display()
                )));
            }
            if blobs.contains_key(&sha) {
                return Err(Error::Plan(format!(
                    "read bundle '{}': duplicate blob '{sha}'",
                    path.display()
                )));
            }
            let mut gzipped = Vec::new();
            entry.read_to_end(&mut gzipped).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            let raw = gunzip_bytes(&gzipped, &sha).map_err(|error| {
                Error::Plan(format!("read bundle '{}': {error}", path.display()))
            })?;
            blobs.insert(sha, raw);
        } else {
            return Err(Error::Plan(format!(
                "read bundle '{}': unexpected entry '{}'",
                path.display(),
                entry_path.display()
            )));
        }
    }
    let Some(stored) = manifest else {
        return Err(Error::Plan(format!(
            "read bundle '{}': missing manifest",
            path.display()
        )));
    };
    let mut load = |sha: &str| match blobs.get(sha) {
        Some(bytes) => Ok(bytes.clone()),
        None => Err(Error::Plan(format!(
            "read bundle '{}': missing blob '{sha}'",
            path.display()
        ))),
    };
    let mut documents = Vec::with_capacity(stored.documents.len());
    for manifest in &stored.documents {
        let data = hydrate_data(&manifest.data, &mut load)?;
        documents.push(Document {
            path: manifest.path.clone(),
            data,
            data_hash: manifest.data_hash.clone(),
        });
    }
    Ok(Plan {
        version: stored.version,
        documents,
        created_at: stored.created_at.clone(),
        hooks: stored.hooks.clone(),
    })
}

/// Loads one plan input holding either a bundle or a manifest.
///
/// Bundle files carry the `.cb` extension and hydrate from
/// the archive alone. Every other path reads as a slot
/// manifest first, then retries as a bundle, so renamed
/// bundles still load.
///
/// # Arguments
///
/// * `path` - the resolved plan file under reading.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live plan holding binary bytes.
///
/// # Errors
///
/// Unreadable files plus bad payloads fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Plan;
/// use confit_core::store::{load_plan_input, write_bundle};
///
/// let fs = MemoryFs::new();
/// let dest = std::path::Path::new("bundle.cb");
/// assert!(matches!(write_bundle(&Plan::empty(), dest, &fs), Ok(())));
/// assert!(matches!(load_plan_input(dest, &fs), Ok(plan) if plan.documents.is_empty()));
/// ```
pub fn load_plan_input(path: &Path, fs: &dyn Filesystem) -> Result<Plan> {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cb"))
    {
        return read_bundle(path, fs);
    }
    match load_state(Some(path), fs) {
        Ok(plan) => Ok(plan),
        Err(first) => match read_bundle(path, fs) {
            Ok(plan) => Ok(plan),
            Err(_) => Err(first),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Plan;

    #[test]
    fn parallel_plan_json_matches_sequential() {
        use crate::document::{StructuredFormat, Table};
        use crate::ids::DocPath;

        let plan = Plan {
            version: PLAN_VERSION,
            documents: vec![
                Document::new(
                    DocPath::new("note"),
                    DocumentData::Text {
                        content: "héllo \"quoted\"\n".to_string(),
                        mode: None,
                    },
                ),
                Document::new(
                    DocPath::new("bin"),
                    DocumentData::Opaque {
                        content: vec![0xFF, 0x00, 0x41],
                        mode: None,
                    },
                ),
                Document::new(
                    DocPath::new("app.json"),
                    DocumentData::Structured {
                        format: StructuredFormat::Json,
                        data: Table::from([("name".to_string(), serde_json::json!("confit"))]),
                    },
                ),
            ],
            created_at: "now".to_string(),
            hooks: vec![crate::hook::Hook {
                argv: vec!["mise".to_string(), "install".to_string()],
                path: Vec::new(),
                when: None,
                checks: Vec::new(),
                timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
            }],
        };
        let parallel = match plan_json(&plan) {
            Ok(text) => text,
            Err(error) => panic!("parallel serializes: {error}"),
        };
        let sequential = match serde_json::to_string_pretty(&Manifest::of(&plan)) {
            Ok(text) => text,
            Err(error) => panic!("sequential serializes: {error}"),
        };
        assert_eq!(parallel, sequential);
        assert!(parallel.contains('\n'));
        assert!(parallel.contains("\"blob\""));
        assert!(!parallel.contains("/wBB"));
    }

    #[test]
    fn named_plan_resolves_under_plans_dir() {
        let path = match resolve_named_plan("work") {
            Ok(path) => path,
            Err(error) => panic!("named plan resolves: {error}"),
        };
        assert!(path.ends_with("confit/plans/work.json"));
    }

    #[test]
    fn named_plan_rejects_empty_separators_and_parent() {
        for name in ["", "a/b", "a\\b", ".", ".."] {
            match resolve_named_plan(name) {
                Ok(_) => panic!("{name:?} passes"),
                Err(error) => assert!(!error.to_string().is_empty()),
            }
        }
    }

    #[test]
    fn named_plan_roundtrips_through_memory_fs() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let built = match Plan::build(
            vec![crate::document::Document::new(
                crate::ids::DocPath::new("note"),
                crate::document::DocumentData::Text {
                    content: "hi".to_string(),
                    mode: None,
                },
            )],
            Vec::new(),
        ) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let dest = match resolve_named_plan("work") {
            Ok(dest) => dest,
            Err(error) => panic!("named plan resolves: {error}"),
        };
        match write_plan(&built, Some(&dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("named plan writes: {error}"),
        }
        let loaded = match load_state(Some(&dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("named plan loads: {error}"),
        };
        assert_eq!(loaded.documents.len(), 1);
    }

    #[test]
    fn hooks_roundtrip_through_memory_fs() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let hooks = vec![crate::hook::Hook {
            argv: vec!["mise".to_string(), "install".to_string()],
            path: vec!["/home/tester/.local/bin".to_string()],
            when: Some(crate::document::Condition::InPath {
                name: "mise".to_string(),
            }),
            checks: vec![crate::document::Condition::InPath {
                name: "bat".to_string(),
            }],
            timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }];
        let built = match Plan::build(Vec::new(), hooks) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("plan loads: {error}"),
        };
        assert_eq!(loaded, built);
        assert_eq!(loaded.hooks.len(), 1);
    }

    #[test]
    fn version_two_state_fails_as_unsupported() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        match fs.write(
            std::path::Path::new("state.json"),
            b"{\"version\":2,\"documents\":[],\"created_at\":\"\",\"hooks\":[]}",
        ) {
            Ok(()) => {}
            Err(error) => panic!("memory writes: {error}"),
        }
        match load_state(Some(std::path::Path::new("state.json")), &fs) {
            Ok(_) => panic!("stale version passes"),
            Err(error) => assert_eq!(
                error.to_string(),
                format!("state version 2 reads unsupported, want {PLAN_VERSION}")
            ),
        }
    }

    fn tree_recorded() -> Vec<Document> {
        use crate::document::TreeMember;
        use crate::ids::DocPath;

        vec![Document::new(
            DocPath::new("fonts"),
            DocumentData::Tree {
                members: vec![
                    TreeMember {
                        rel: "kept.ttf".into(),
                        content: vec![1],
                        mode: 0o644,
                    },
                    TreeMember {
                        rel: "gone.ttf".into(),
                        content: vec![2],
                        mode: 0o644,
                    },
                ],
            },
        )]
    }

    #[test]
    fn write_tree_members_land_with_modes() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        match write_documents(&tree_recorded(), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("tree writes: {error}"),
        }
        let dest = std::path::Path::new("fonts");
        match fs.read(&dest.join("kept.ttf")) {
            Ok(bytes) => assert_eq!(bytes, vec![1]),
            Err(error) => panic!("member reads: {error}"),
        }
        assert_eq!(fs.file_mode(&dest.join("kept.ttf")), Some(0o644));
        assert!(fs.exists(&dest.join("gone.ttf")));
    }

    #[test]
    fn remove_tree_members_drops_only_dropped() {
        use crate::document::TreeMember;
        use crate::fs::{Filesystem, MemoryFs};
        use crate::ids::DocPath;

        let fs = MemoryFs::new();
        match write_documents(&tree_recorded(), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("tree writes: {error}"),
        }
        let dest = std::path::Path::new("fonts");
        match fs.write(&dest.join("hand.ttf"), b"mine") {
            Ok(()) => {}
            Err(error) => panic!("hand writes: {error}"),
        }
        let desired = vec![Document::new(
            DocPath::new("fonts"),
            DocumentData::Tree {
                members: vec![TreeMember {
                    rel: "kept.ttf".into(),
                    content: vec![1],
                    mode: 0o644,
                }],
            },
        )];
        match remove_tree_members(&tree_recorded(), &desired, &fs) {
            Ok(removed) => assert_eq!(removed, 1),
            Err(error) => panic!("members remove: {error}"),
        }
        assert!(fs.exists(&dest.join("kept.ttf")));
        assert!(!fs.exists(&dest.join("gone.ttf")));
        assert!(fs.exists(&dest.join("hand.ttf")));
    }

    #[test]
    fn remove_orphans_skips_tree_destinations() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        match write_documents(&tree_recorded(), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("tree writes: {error}"),
        }
        match remove_orphans(&tree_recorded(), &[], &fs) {
            Ok(removed) => assert_eq!(removed, 0),
            Err(error) => panic!("orphans remove: {error}"),
        }
        let dest = std::path::Path::new("fonts");
        assert!(fs.exists(&dest.join("kept.ttf")));
    }

    fn mixed_plan() -> Plan {
        use crate::document::{Document, DocumentData, TreeMember};
        use crate::ids::DocPath;

        let documents = vec![
            Document::new(
                DocPath::new("note"),
                DocumentData::Text {
                    content: "hi".to_string(),
                    mode: None,
                },
            ),
            Document::new(
                DocPath::new("bin"),
                DocumentData::Opaque {
                    content: vec![0xFF, 0x00, 0x41],
                    mode: None,
                },
            ),
            Document::new(
                DocPath::new("fonts"),
                DocumentData::Tree {
                    members: vec![
                        TreeMember {
                            rel: "a.ttf".into(),
                            content: vec![1, 2, 3],
                            mode: 0o644,
                        },
                        TreeMember {
                            rel: "b.ttf".into(),
                            content: vec![4, 5, 6],
                            mode: 0o644,
                        },
                    ],
                },
            ),
        ];
        match Plan::build(documents, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        }
    }

    fn pool_blobs(fs: &crate::fs::MemoryFs) -> Vec<std::path::PathBuf> {
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
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("plan loads: {error}"),
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
        use crate::document::{Document, DocumentData, TreeMember};
        use crate::ids::DocPath;

        let fs = crate::fs::MemoryFs::new();
        let shared = vec![9, 9, 9];
        let documents = vec![
            Document::new(
                DocPath::new("first"),
                DocumentData::Opaque {
                    content: shared.clone(),
                    mode: None,
                },
            ),
            Document::new(
                DocPath::new("second"),
                DocumentData::Opaque {
                    content: shared.clone(),
                    mode: None,
                },
            ),
            Document::new(
                DocPath::new("fonts"),
                DocumentData::Tree {
                    members: vec![TreeMember {
                        rel: "a.ttf".into(),
                        content: shared.clone(),
                        mode: 0o644,
                    }],
                },
            ),
        ];
        let built = match Plan::build(documents, Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("plan builds: {error}"),
        };
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
        }
        let blobs = pool_blobs(&fs);
        assert_eq!(blobs.len(), 1);
        assert_eq!(gunzip_blob(&fs, &blobs[0]), shared);
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("plan loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn prune_blobs_drops_only_unreferenced() {
        use crate::document::{Document, DocumentData};
        use crate::fs::{Filesystem, MemoryFs};
        use crate::ids::DocPath;

        fn opaque_plan(path: &str, byte: u8) -> Plan {
            match Plan::build(
                vec![Document::new(
                    DocPath::new(path),
                    DocumentData::Opaque {
                        content: vec![byte],
                        mode: None,
                    },
                )],
                Vec::new(),
            ) {
                Ok(built) => built,
                Err(error) => panic!("plan builds: {error}"),
            }
        }

        let fs = MemoryFs::new();
        let slot = match default_state_path() {
            Ok(slot) => slot,
            Err(error) => panic!("slot resolves: {error}"),
        };
        let slot_plan = opaque_plan("slot-bin", 10);
        match write_plan(&slot_plan, Some(&slot), &fs) {
            Ok(()) => {}
            Err(error) => panic!("slot writes: {error}"),
        }
        let previous = match resolve_previous_dir() {
            Ok(previous) => previous,
            Err(error) => panic!("history resolves: {error}"),
        };
        let history_plan = opaque_plan("history-bin", 20);
        match write_plan(&history_plan, Some(&previous.join("1.json")), &fs) {
            Ok(()) => {}
            Err(error) => panic!("history writes: {error}"),
        }
        let named = match resolve_named_plan("work") {
            Ok(named) => named,
            Err(error) => panic!("named resolves: {error}"),
        };
        let named_plan = opaque_plan("named-bin", 30);
        match write_plan(&named_plan, Some(&named), &fs) {
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
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
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
            Err(error) => panic!("plan loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn lazy_hydration_loads_pool_on_mismatched_disk() {
        use crate::fs::{Filesystem, MemoryFs};

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
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
            Err(error) => panic!("plan loads: {error}"),
        };
        assert_eq!(loaded, built);
    }

    #[test]
    fn lazy_hydration_rejects_corrupt_pool_blob() {
        use crate::fs::{Filesystem, MemoryFs};
        use std::io::Write as _;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
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
        let dest = std::path::Path::new("plan.json");
        match write_plan(&built, Some(dest), &fs) {
            Ok(()) => {}
            Err(error) => panic!("plan writes: {error}"),
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
    fn bundle_roundtrip_restores_plan_with_referenced_blobs_only() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let built = mixed_plan();
        let dest = std::path::Path::new("bundle.tgz");
        match write_bundle(&built, dest, &fs) {
            Ok(()) => {}
            Err(error) => panic!("bundle writes: {error}"),
        }
        let restored = match read_bundle(dest, &fs) {
            Ok(restored) => restored,
            Err(error) => panic!("bundle reads: {error}"),
        };
        assert_eq!(restored, built);
        let bytes = match fs.read(dest) {
            Ok(bytes) => bytes,
            Err(error) => panic!("bundle reads: {error}"),
        };
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        let entries = match archive.entries() {
            Ok(entries) => entries,
            Err(error) => panic!("bundle lists: {error}"),
        };
        let mut names = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => panic!("entry reads: {error}"),
            };
            let path = match entry.path() {
                Ok(path) => path.into_owned(),
                Err(error) => panic!("entry names: {error}"),
            };
            names.push(path.to_string_lossy().into_owned());
        }
        names.sort();
        let mut want = vec!["manifest.json".to_string()];
        for raw in [vec![0xFF, 0x00, 0x41], vec![1, 2, 3], vec![4, 5, 6]] {
            want.push(format!("blobs/{}", crate::plan::sha256_hex(&raw)));
        }
        want.sort();
        assert_eq!(names, want);
    }
}
