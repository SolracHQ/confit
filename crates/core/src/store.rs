//! Store
//!
//! Plan files plus document writes plus history rotation.

use std::path::{Path, PathBuf};

use crate::document::{Document, DocumentData};
use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::ids::DocPath;
use crate::plan::{PLAN_VERSION, Plan};

/// One stored plan entry for the recover listing.
///
/// # Examples
///
/// ```text
/// use confit_core::store::PreviousEntry;
///
/// let entry = PreviousEntry { index: 0, created_at: String::new() };
/// assert!(matches!(entry.index, 0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviousEntry {
    /// Holds the listing position used as the recover pick.
    pub index: usize,
    /// Holds the stored plan creation timestamp.
    pub created_at: String,
}

/// Loads previous plan, treating missing files as empty.
///
/// Plan JSON parses in two steps. A version probe runs
/// first. Missing or invalid versions fail. Mismatched
/// versions fail as unsupported. Hashes persist in the
/// file and read trusted, so loads skip rendering.
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
/// mismatch fail as plan errors.
///
/// # Examples
///
/// ```text
/// use confit_core::fs::OsFs;
/// use confit_core::plan::PLAN_VERSION;
/// use confit_core::store::load_state;
///
/// let outcome = load_state(None, &OsFs);
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
    let plan: Plan = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", file.display())))?;
    if plan.version != PLAN_VERSION {
        return Err(Error::Plan(format!(
            "state version {} reads unsupported, want {PLAN_VERSION}",
            plan.version
        )));
    }
    log::debug!(
        "read took {}ms for {}",
        read_start.elapsed().as_millis(),
        file.display()
    );
    Ok(plan)
}

/// Writes the plan payload to a file or stdout.
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
/// ```text
/// use confit_core::fs::OsFs;
/// use confit_core::plan::{PLAN_VERSION, Plan};
/// use confit_core::store::write_plan;
///
/// let plan = Plan { version: PLAN_VERSION, documents: Vec::new(), created_at: String::new() };
/// assert!(matches!(write_plan(&plan, None, &OsFs), Ok(())));
/// ```
/// Serializes one plan with per-document parallelism.
///
/// Documents serialize independently across rayon threads,
/// then join in path order. Output bytes match sequential
/// serde exactly, keeping every reader unchanged.
///
/// # Arguments
///
/// * `plan` - the plan under serializing.
///
/// # Returns
///
/// The compact plan JSON.
///
/// # Errors
///
/// Document serialization failures surface as plan errors.
///
/// # Examples
///
/// ```text
/// use confit_core::plan::Plan;
/// use confit_core::store::plan_json;
///
/// let plan = Plan::empty();
/// assert!(matches!(plan_json(&plan), Ok(text) if text.contains("documents")));
/// ```
pub fn plan_json(plan: &Plan) -> Result<String> {
    use rayon::prelude::*;
    let bodies = plan
        .documents
        .par_iter()
        .map(serde_json::to_string)
        .collect::<std::result::Result<Vec<String>, serde_json::Error>>()
        .map_err(|error| Error::Plan(format!("render plan: {error}")))?;
    let created = serde_json::to_string(&plan.created_at)
        .map_err(|error| Error::Plan(format!("render plan: {error}")))?;
    Ok(format!(
        "{{\"version\":{},\"documents\":[{}],\"created_at\":{}}}",
        plan.version,
        bodies.join(","),
        created
    ))
}

/// Writes the plan payload to a file or stdout.
///
/// Both destinations take the parallel compact form.
pub fn write_plan(plan: &Plan, out: Option<&Path>, fs: &dyn Filesystem) -> Result<()> {
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
/// ```text
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::fs::{Filesystem, MemoryFs};
/// use confit_core::ids::DocPath;
/// use confit_core::store::write_documents;
///
/// let fs = MemoryFs::new();
/// let documents = vec![Document::new(
///     DocPath::new("note"),
///     DocumentData::Text { content: "hi".into() },
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
            _ => match document.bytes() {
                Ok(bytes) => fs.write(&expanded, &bytes),
                Err(error) => return Err(error),
            },
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

/// Removes recorded paths absent from desired documents.
///
/// Only state-recorded paths delete, never anything else.
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
/// ```text
/// use confit_core::document::{Document, DocumentData};
/// use confit_core::fs::MemoryFs;
/// use confit_core::ids::DocPath;
/// use confit_core::store::remove_orphans;
///
/// let fs = MemoryFs::new();
/// let recorded = vec![Document::new(
///     DocPath::new("gone"),
///     DocumentData::Text { content: "hi".into() },
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

/// Stored plans kept before rotation drops the oldest.
const PREVIOUS_KEPT: usize = 5;

/// Lists stored plans oldest first with recover indices.
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
/// ```text
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
        .map(|(index, (_, stored))| PreviousEntry {
            index,
            created_at: stored.created_at,
        })
        .collect())
}

/// Reads stored plans oldest first with their file paths.
///
/// Unreadable files plus bad JSON plus stale versions skip
/// quietly. Missing folders read as empty.
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
/// ```text
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
    let mut out = Vec::new();
    for file in files {
        let bytes = match fs.read(&file) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let stored: Plan = match serde_json::from_slice(&bytes) {
            Ok(stored) => stored,
            Err(_) => continue,
        };
        if stored.version != PLAN_VERSION {
            continue;
        }
        out.push((file, stored));
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
/// ```text
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
/// ```text
/// use confit_core::store::resolve_previous_dir;
///
/// let dir = resolve_previous_dir();
/// assert!(matches!(dir, Ok(dir) if dir.ends_with("confit/previous")));
/// ```
pub fn resolve_previous_dir() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("previous"))
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
/// ```text
/// use confit_core::store::default_state_path;
///
/// let slot = default_state_path();
/// assert!(matches!(slot, Ok(slot) if slot.ends_with("confit/state.json")));
/// ```
pub fn default_state_path() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("state.json"))
}

/// Resolves the effective state file for one run.
///
/// Explicit flags win. Omitted flags fall back to the fixed slot.
///
/// # Arguments
///
/// * `state` - the explicit state file, holding `None` for the fixed slot.
///
/// # Returns
///
/// The state file gaining the new plan.
///
/// # Errors
///
/// Folder resolution failures surface as plan errors.
///
/// # Examples
///
/// ```text
/// use confit_core::store::resolve_state_file;
/// use std::path::Path;
///
/// let path = resolve_state_file(Some(Path::new("custom.json")));
/// assert!(matches!(path, Ok(path) if path == std::path::PathBuf::from("custom.json")));
/// ```
pub fn resolve_state_file(state: Option<&Path>) -> Result<PathBuf> {
    match state {
        Some(file) => Ok(file.to_path_buf()),
        None => default_state_path(),
    }
}

/// Stores one applied plan, rotating past the kept count.
///
/// Stamp names sort oldest first. Colliding stamps bump up
/// by one until the name reads fresh.
///
/// # Arguments
///
/// * `plan` - the applied plan under storing.
/// * `fs` - the backend under writing.
///
/// # Returns
///
/// The stored plan path backing recover.
///
/// # Errors
///
/// Clock plus write failures surface as plan or io errors.
///
/// # Examples
///
/// ```text
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Plan;
/// use confit_core::store::archive_previous;
///
/// let outcome = archive_previous(&Plan::empty(), &MemoryFs::new());
/// assert!(matches!(outcome, Ok(_)));
/// ```
pub fn archive_previous(plan: &Plan, fs: &dyn Filesystem) -> Result<PathBuf> {
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
        };
        let parallel = match plan_json(&plan) {
            Ok(text) => text,
            Err(error) => panic!("parallel serializes: {error}"),
        };
        let sequential = match serde_json::to_string(&plan) {
            Ok(text) => text,
            Err(error) => panic!("sequential serializes: {error}"),
        };
        assert_eq!(parallel, sequential);
    }
}
