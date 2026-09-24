//! Memory
//!
//! Memory workspace for tests.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::document::{ManifestData, ManifestDocument};
use confit_core::error::{Error, Result};
use confit_core::fs::snapshot::TreeMemberRead;
use confit_core::handles::{ResourceHandle, Route, RouteBase, TrustedHandle};
use confit_core::ids::{ReadOutcome, sha256_hex};

use super::Workspace;
use super::run_secret_command;
use crate::blob::BlobStore;

/// Memory workspace for tests.
///
/// Files ride an in-memory map keyed by resolved route.
/// Text, structured, link, and rc payloads write; opaque
/// and tree payloads stream from the paired blob store;
/// secret payloads execute their command at apply time.
#[derive(Debug, Default)]
pub struct MemoryWorkspace {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
}

impl MemoryWorkspace {
    /// Builds an empty memory workspace.
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
        }
    }

    /// Seeds one file behind a resolved path.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(path.to_path_buf(), bytes.to_vec());
    }
}

/// Expands one destination route against fake memory roots.
///
/// Each base maps to a fixed fake folder, so tests seed
/// and assert under stable paths. Literal carries its
/// path verbatim.
fn resolve_memory(route: &Route) -> PathBuf {
    let relative = route.relative();
    match route.base() {
        RouteBase::Home => PathBuf::from("memory-home").join(relative),
        RouteBase::Config => PathBuf::from("memory-config").join(relative),
        RouteBase::Data => PathBuf::from("memory-data").join(relative),
        RouteBase::Cache => PathBuf::from("memory-cache").join(relative),
        RouteBase::Literal => relative.to_path_buf(),
    }
}

impl Workspace for MemoryWorkspace {
    fn resolve(&self, route: &Route) -> PathBuf {
        resolve_memory(route)
    }

    fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle> {
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        let guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(path) {
            Some(bytes) => ResourceHandle::new(exec_root, path.to_path_buf(), sha256_hex(bytes)),
            None => Err(Error::Plan(format!(
                "cannot read '{}': missing file",
                path.display()
            ))),
        }
    }

    fn snapshot_doc(&self, document: &ManifestDocument) -> ReadOutcome {
        let expanded = self.resolve(&document.destination);
        let guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.get(&expanded) {
            Some(bytes) => ReadOutcome::Present {
                bytes: bytes.clone(),
                mode: None,
            },
            None => ReadOutcome::Absent,
        }
    }

    fn snapshot_tree(&self, document: &ManifestDocument) -> BTreeMap<String, TreeMemberRead> {
        let dir = self.resolve(&document.destination);
        let guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut out = BTreeMap::new();
        for (file, bytes) in guard.iter() {
            let Ok(rel) = file.strip_prefix(&dir) else {
                continue;
            };
            let Some(name) = rel.to_str() else {
                continue;
            };
            out.insert(
                name.to_string(),
                TreeMemberRead::Present {
                    bytes: bytes.clone(),
                    mode: None,
                },
            );
        }
        out
    }

    fn write_documents(
        &self,
        documents: &[ManifestDocument],
        blobs: &dyn BlobStore,
        changed: &BTreeSet<Route>,
        on_written: Option<&dyn Fn(&Route)>,
    ) -> Result<usize> {
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut written = 0;
        for document in documents {
            let expanded = self.resolve(&document.destination);
            if document.data.unmanaged()
                && guard.contains_key(&expanded)
                && !changed.contains(&document.destination)
            {
                continue;
            }
            let bytes = match &document.data {
                ManifestData::Text { content, .. } => Some(content.clone().into_bytes()),
                ManifestData::Link { target } => Some(target.clone().into_bytes()),
                ManifestData::Structured { .. } | ManifestData::Rc(_) => {
                    Some(document.render(&|route| self.resolve(route))?)
                }
                ManifestData::Opaque { blob, .. } => Some(read_blob(blobs, blob)?),
                ManifestData::Tree { members } => {
                    for member in members {
                        let bytes = read_blob(blobs, &member.blob)?;
                        guard.insert(expanded.join(&member.relative), bytes);
                    }
                    None
                }
                ManifestData::Secret { argv, .. } => {
                    drop(guard);
                    let bytes = run_secret_command(argv, &document.destination, &|route| {
                        self.resolve(route)
                    })?;
                    guard = match self.files.lock() {
                        Ok(guard) => guard,
                        Err(poisoned) => poisoned.into_inner(),
                    };
                    Some(bytes)
                }
            };
            if let Some(bytes) = bytes {
                guard.insert(expanded, bytes);
                if let Some(notify) = on_written {
                    notify(&document.destination);
                }
                written += 1;
            } else {
                if let Some(notify) = on_written {
                    notify(&document.destination);
                }
                written += members_written(&document.data);
            }
        }
        Ok(written)
    }

    fn remove_orphans(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize> {
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut removed = 0;
        for old in recorded {
            if old.data.tree_members().is_some() {
                continue;
            }
            let kept = desired
                .iter()
                .any(|document| document.destination == old.destination);
            if kept {
                continue;
            }
            if guard.remove(&self.resolve(&old.destination)).is_some() {
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn remove_tree_members(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize> {
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut removed = 0;
        for old in recorded {
            let Some(old_members) = old.data.tree_members() else {
                continue;
            };
            let new_rels: BTreeSet<&str> = desired
                .iter()
                .filter(|document| document.destination == old.destination)
                .filter_map(|document| document.data.tree_members())
                .flat_map(|members| members.iter().map(|member| member.relative.as_str()))
                .collect();
            let dest = self.resolve(&old.destination);
            for member in old_members {
                if new_rels.contains(member.relative.as_str()) {
                    continue;
                }
                if guard.remove(&dest.join(&member.relative)).is_some() {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }

    fn scaffold(&self, dest: &Route, name: &str) -> Result<()> {
        let _ = name;
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(self.resolve(dest), Vec::new());
        Ok(())
    }

    fn read_profile(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(&self.files, handle.canonical())
    }

    fn read_module(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(&self.files, handle.canonical())
    }
}

/// Counts tree members as written documents.
fn members_written(data: &ManifestData) -> usize {
    match data {
        ManifestData::Tree { members } => members.len(),
        _ => 0,
    }
}

/// Streams one blob handle into bytes.
fn read_blob(blobs: &dyn BlobStore, handle: &confit_core::handles::BlobHandle) -> Result<Vec<u8>> {
    let mut reader = blobs.open(handle)?;
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| Error::Plan(format!("read blob '{}': {error}", handle.sha())))?;
    Ok(bytes)
}

/// Reads one memory file as text.
fn read_text(files: &Mutex<HashMap<PathBuf, Vec<u8>>>, path: &Path) -> Result<String> {
    let guard = match files.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    match guard.get(path) {
        Some(bytes) => String::from_utf8(bytes.clone())
            .map_err(|_| Error::Plan(format!("cannot read '{}': invalid text", path.display()))),
        None => Err(Error::Plan(format!(
            "cannot read '{}': missing file",
            path.display()
        ))),
    }
}
