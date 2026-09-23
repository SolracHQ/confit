//! Workspace
//!
//! Disk snapshots, document writes, scaffolding, and text reads.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use confit_core::document::ManifestData;
use confit_core::document::ManifestDocument;
use confit_core::error::{Error, Result};
use confit_core::fs::snapshot::TreeMemberRead;
use confit_core::ids::{DocPath, ReadOutcome};
use confit_core::store::blobs::BlobRef;

/// Managed disk behind snapshots, writes, and text reads.
///
/// Snapshot outcomes mirror the drift shapes: absent for
/// missing paths, present bytes and mode else.
pub trait Workspace {
    /// Snapshots one document path through its kind-aware reader.
    fn snapshot_doc(&self, path: &DocPath) -> ReadOutcome;

    /// Snapshots one tree destination into relative member reads.
    fn snapshot_tree(&self, path: &DocPath) -> BTreeMap<String, TreeMemberRead>;

    /// Writes every document to its expanded path.
    ///
    /// # Errors
    ///
    /// Render and io failures surface as plan or io errors.
    fn write_documents(
        &self,
        documents: &[ManifestDocument],
        blobs: &BTreeMap<String, BlobRef>,
    ) -> Result<usize>;

    /// Removes recorded paths absent from desired documents.
    ///
    /// # Errors
    ///
    /// Removal failures surface as io errors.
    fn remove_orphans(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize>;

    /// Removes dropped tree members between recorded and desired manifests.
    ///
    /// # Errors
    ///
    /// Removal failures surface as io errors.
    fn remove_tree_members(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize>;

    /// Scaffolds one project holding profiles and stubs.
    ///
    /// # Errors
    ///
    /// Write failures surface as plan or io errors.
    fn scaffold(&self, dest: &Path, name: &str) -> Result<()>;

    /// Appends one line to the hook log.
    ///
    /// # Errors
    ///
    /// Write failures surface as plan or io errors.
    fn append_hook_log(&self, line: &str) -> Result<()>;

    /// Reads one profile file as text.
    ///
    /// # Errors
    ///
    /// Missing files and invalid text fail as plan errors.
    fn read_profile(&self, path: &Path) -> Result<String>;

    /// Reads one module file as text.
    ///
    /// # Errors
    ///
    /// Missing files and invalid text fail as plan errors.
    fn read_module(&self, path: &Path) -> Result<String>;
}

/// Memory workspace for tests.
///
/// Files ride an in-memory map keyed by expanded path.
/// Text and link payloads write; opaque and tree payloads
/// skip until real backends land.
#[derive(Debug, Default)]
pub struct MemoryWorkspace {
    files: Mutex<HashMap<PathBuf, Vec<u8>>>,
    hook_log: Mutex<Vec<String>>,
}

impl MemoryWorkspace {
    /// Builds an empty memory workspace.
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            hook_log: Mutex::new(Vec::new()),
        }
    }

    /// Seeds one file behind an expanded path.
    pub fn insert(&self, path: &Path, bytes: &[u8]) {
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(path.to_path_buf(), bytes.to_vec());
    }
}

impl Workspace for MemoryWorkspace {
    fn snapshot_doc(&self, path: &DocPath) -> ReadOutcome {
        let expanded = path.expand();
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

    fn snapshot_tree(&self, path: &DocPath) -> BTreeMap<String, TreeMemberRead> {
        let dir = path.expand();
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
        blobs: &BTreeMap<String, BlobRef>,
    ) -> Result<usize> {
        let _ = blobs;
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut written = 0;
        for document in documents {
            let expanded = document.path.expand();
            if document.data.unmanaged() && guard.contains_key(&expanded) {
                continue;
            }
            let bytes = match &document.data {
                ManifestData::Text { content, .. } => Some(content.clone().into_bytes()),
                ManifestData::Link { target } => Some(target.clone().into_bytes()),
                ManifestData::Structured { .. }
                | ManifestData::Rc(_)
                | ManifestData::Opaque { .. }
                | ManifestData::Tree { .. } => None,
            };
            if let Some(bytes) = bytes {
                guard.insert(expanded, bytes);
                written += 1;
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
            let kept = desired.iter().any(|document| document.path == old.path);
            if kept {
                continue;
            }
            if guard.remove(&old.path.expand()).is_some() {
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
                .filter(|document| document.path == old.path)
                .filter_map(|document| document.data.tree_members())
                .flat_map(|members| members.iter().map(|member| member.relative.as_str()))
                .collect();
            let dest = old.path.expand();
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

    fn scaffold(&self, dest: &Path, name: &str) -> Result<()> {
        let _ = name;
        let mut guard = match self.files.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.insert(dest.to_path_buf(), Vec::new());
        Ok(())
    }

    fn append_hook_log(&self, line: &str) -> Result<()> {
        let mut guard = match self.hook_log.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        guard.push(line.to_string());
        Ok(())
    }

    fn read_profile(&self, path: &Path) -> Result<String> {
        read_text(&self.files, path)
    }

    fn read_module(&self, path: &Path) -> Result<String> {
        read_text(&self.files, path)
    }
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
