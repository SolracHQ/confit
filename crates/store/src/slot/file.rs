//! File
//!
//! File-backed slot store.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use confit_core::error::{Error, Result};
use confit_core::handles::BlobHandle;
use confit_core::plan::{BUNDLE_VERSION, Bundle};
use confit_core::progress::ProgressSender;
use confit_core::store::SlotKind;
use confit_core::store::manifest::{HistoryEntry, Manifest, manifest_json};

use super::SlotStore;
use crate::StoreRoots;

/// Stored plans kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 5;

/// State file name under the config base.
const STATE_FILE: &str = "state.json";

/// History folder name under the config base.
const PREVIOUS_DIR: &str = "previous";

/// Named slot folder name under the config base.
const PLANS_DIR: &str = "plans";

/// File-backed slot store.
///
/// Applied, named, and history bundles ride config base files.
#[derive(Debug, Clone)]
pub struct FileSlotStore {
    state: PathBuf,
    previous: PathBuf,
    plans: PathBuf,
}

impl FileSlotStore {
    /// Builds a file-backed slot store under the config base.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            state: roots.config_base.join(STATE_FILE),
            previous: roots.config_base.join(PREVIOUS_DIR),
            plans: roots.config_base.join(PLANS_DIR),
        }
    }

    /// Reads the slot path for one validated name.
    ///
    /// # Errors
    ///
    /// Empty names, separator carriers, and dot segments
    /// fail as plan errors.
    fn named_slot(&self, name: &str) -> Result<PathBuf> {
        check_slot_name(name)?;
        Ok(self.plans.join(format!("{name}.json")))
    }
}

impl SlotStore for FileSlotStore {
    fn load(&self) -> Result<Bundle> {
        load_bundle(&self.state)
    }

    fn store(&self, bundle: &Bundle, progress: Option<&ProgressSender>) -> Result<PathBuf> {
        let _ = progress;
        let text = manifest_json(&bundle.manifest)?;
        write_text(&self.state, &text)?;
        let mut stamp = system_nanos()?;
        let mut dest = self.previous.join(format!("{stamp}.json"));
        while dest.exists() {
            stamp += 1;
            dest = self.previous.join(format!("{stamp}.json"));
        }
        write_text(&dest, &text)?;
        rotate_history(&self.previous)?;
        Ok(dest)
    }

    fn resolve(&self, picker: Option<&str>) -> Result<(Bundle, SlotKind)> {
        let Some(raw) = picker else {
            if !self.state.exists() {
                return Err(Error::Plan(
                    "the applied slot reads absent, apply first".to_string(),
                ));
            }
            return Ok((load_bundle(&self.state)?, SlotKind::Applied));
        };
        if let Some(name) = raw.strip_prefix('@') {
            let path = self.named_slot(name)?;
            if !path.exists() {
                return Err(Error::Plan(format!("'@{name}' reads absent")));
            }
            return Ok((load_bundle(&path)?, SlotKind::Named(name.to_string())));
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = rest.parse().map_err(|_| {
                Error::Plan(format!(
                    "'{raw}' reads unsupported, want '%N' holding a number from 1"
                ))
            })?;
            let entries = stored_bundles(&self.previous)?;
            let total = entries.len();
            if pick < 1 || pick > total {
                return Err(Error::Plan(format!(
                    "'{raw}' reads out of range, holding {total} stored manifests"
                )));
            }
            let (_, bundle) = entries.into_iter().nth(pick - 1).ok_or_else(|| {
                Error::Plan(format!(
                    "'{raw}' reads out of range, holding {total} stored manifests"
                ))
            })?;
            return Ok((bundle, SlotKind::History(pick)));
        }
        Err(Error::Plan(format!(
            "'{raw}' reads unsupported, want '%N', '@name', or nothing"
        )))
    }

    fn list_history(&self) -> Result<Vec<HistoryEntry>> {
        Ok(stored_bundles(&self.previous)?
            .into_iter()
            .enumerate()
            .map(|(position, _)| HistoryEntry {
                index: position + 1,
            })
            .collect())
    }

    fn store_named(&self, name: &str, bundle: &Bundle) -> Result<()> {
        let path = self.named_slot(name)?;
        let text = manifest_json(&bundle.manifest)?;
        write_text(&path, &text)?;
        Ok(())
    }

    fn delete_named(&self, name: &str) -> Result<()> {
        let path = self.named_slot(name)?;
        if !path.exists() {
            return Err(Error::Plan(format!("'@{name}' reads absent")));
        }
        std::fs::remove_file(&path).map_err(Error::from)?;
        Ok(())
    }

    fn is_first_run(&self) -> bool {
        !self.state.exists()
    }
}

/// Loads one manifest file with blob handle resolution.
///
/// Missing files read as empty. Handles carry content
/// identity, so no disk or pool check runs here.
///
/// # Errors
///
/// Unreadable present files, bad JSON, version
/// mismatch and malformed blob hashes fail as plan errors.
fn load_bundle(path: &Path) -> Result<Bundle> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Bundle::empty());
        }
        Err(error) => return Err(Error::from(error)),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", path.display())))?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(version) if version == u64::from(BUNDLE_VERSION) => {}
        Some(version) => {
            return Err(Error::Plan(format!(
                "state version {version} reads unsupported, want {BUNDLE_VERSION}"
            )));
        }
        None => {
            return Err(Error::Plan(format!(
                "read state '{}': missing manifest version",
                path.display()
            )));
        }
    }
    let stored: Manifest = serde_json::from_value(value)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", path.display())))?;
    Ok(hydrate_bundle(&stored))
}

/// Rebuilds one bundle with handle-only blob resolution.
///
/// Blob handles carry content plus stored identity, so resolution
/// clones manifest handles without touching disk.
fn hydrate_bundle(stored: &Manifest) -> Bundle {
    let mut blobs: BTreeMap<String, BlobHandle> = BTreeMap::new();
    for document in &stored.documents {
        for handle in document.data.blob_handles() {
            blobs
                .entry(handle.sha().hex())
                .or_insert_with(|| handle.clone());
        }
    }
    Bundle {
        manifest: stored.clone(),
        blobs,
    }
}

/// Reads stored manifests newest first with their file paths.
///
/// Stamp names stay oldest-first on disk while presentation
/// reverses. Unreadable files, bad JSON, stale versions,
/// and unresolvable blobs skip quietly. Missing folders
/// read as empty.
///
/// # Errors
///
/// Folder listing failures beyond missing folders surface
/// as io errors.
fn stored_bundles(dir: &Path) -> Result<Vec<(PathBuf, Bundle)>> {
    let mut files = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(Error::from)?
            .into_iter()
            .map(|entry| entry.path())
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::from(error)),
    };
    files.sort();
    files.reverse();
    let mut out = Vec::new();
    for file in files {
        let bytes = match std::fs::read(&file) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let stored: Manifest = match serde_json::from_slice(&bytes) {
            Ok(stored) => stored,
            Err(_) => continue,
        };
        if stored.version != BUNDLE_VERSION {
            continue;
        }
        let bundle = hydrate_bundle(&stored);
        out.push((file, bundle));
    }
    Ok(out)
}

/// Lists stored files oldest first.
///
/// Missing folders read as empty.
///
/// # Errors
///
/// Folder listing failures beyond missing folders surface
/// as io errors.
fn history_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .collect::<std::io::Result<Vec<_>>>()
            .map_err(Error::from)?
            .into_iter()
            .map(|entry| entry.path())
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(Error::from(error)),
    };
    files.sort();
    Ok(files)
}

/// Drops stored manifests past the kept count, oldest first.
///
/// # Errors
///
/// Listing and removal failures surface as plan or io errors.
fn rotate_history(dir: &Path) -> Result<()> {
    let files = history_files(dir)?;
    if files.len() > HISTORY_KEPT {
        for stale in files.iter().take(files.len() - HISTORY_KEPT) {
            std::fs::remove_file(stale).map_err(Error::from)?;
        }
    }
    Ok(())
}

/// Writes manifest text to one path, creating parents.
///
/// # Errors
///
/// Parent and write failures surface as plan errors naming
/// the destination.
fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", path.display())))?;
    }
    std::fs::write(path, text.as_bytes())
        .map_err(|error| Error::Plan(format!("cannot write '{}': {error}", path.display())))
}

/// Reads wall-clock nanos for sortable archive file names.
///
/// # Errors
///
/// Clock readings before the epoch fail as plan errors.
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

/// Checks one slot name holds one file stem with no separators.
///
/// Empty names, separator carriers, and dot segments fail.
///
/// # Errors
///
/// Empty names, separator carriers, and dot segments
/// fail as plan errors.
fn check_slot_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::Plan("slot name reads empty".to_string()));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(Error::Plan(format!("slot name '{name}' holds separators")));
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(Error::Plan(format!("slot name '{name}' reads unsupported")));
    }
    if Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Error::Plan(format!("slot name '{name}' reads unsupported")));
    }
    Ok(())
}
