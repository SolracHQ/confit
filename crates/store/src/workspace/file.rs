//! File
//!
//! File-backed workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use confit_core::document::{ManifestData, ManifestDocument, ManifestMember};
use confit_core::error::{Error, Result};
use confit_core::fs::snapshot::TreeMemberRead;
use confit_core::handles::{BlobHandle, ResourceHandle, Route, RouteBase, TrustedHandle};
use confit_core::ids::{ReadOutcome, sha256_read};

use super::Workspace;
use super::run_secret_command;
use crate::StoreRoots;
use crate::blob::BlobStore;

/// Scaffolded profile file name under the project folder.
const PROFILE_FILE: &str = "profile.lua";

/// Scaffolded stubs folder name under the project folder.
const STUBS_DIR: &str = "stubs";

/// File-backed workspace behind snapshots, writes, and text reads.
#[derive(Debug, Clone)]
pub struct FileWorkspace {
    /// Holds the store roots backing construction.
    pub roots: StoreRoots,
}

impl FileWorkspace {
    /// Builds a file-backed workspace under explicit roots.
    ///
    /// Roots arrive explicit from store construction.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            roots: roots.clone(),
        }
    }
}

impl Workspace for FileWorkspace {
    fn resolve(&self, route: &Route) -> PathBuf {
        resolve_host(route)
    }

    fn resource(&self, exec_root: &Path, path: &Path) -> Result<ResourceHandle> {
        if !path.starts_with(exec_root) {
            return Err(Error::Plan(format!(
                "resource path '{}' escapes exec root '{}'",
                path.display(),
                exec_root.display()
            )));
        }
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Error::Plan(format!(
                    "cannot read '{}': missing file",
                    path.display()
                )));
            }
            Err(error) => return Err(Error::from(error)),
        };
        let sha = sha256_read(&mut file)?;
        ResourceHandle::new(exec_root, path.to_path_buf(), sha)
    }

    fn snapshot_doc(&self, document: &ManifestDocument) -> ReadOutcome {
        let expanded = self.resolve(&document.destination);
        let target = match std::fs::read_link(&expanded) {
            Ok(link) => join_link_target(&expanded, &link),
            Err(_) => expanded.clone(),
        };
        if matches!(document.data, ManifestData::Link { .. }) {
            return match std::fs::read_link(&expanded) {
                Ok(link) => ReadOutcome::Present {
                    bytes: link.as_os_str().as_encoded_bytes().to_vec(),
                    mode: None,
                },
                Err(_) => match std::fs::read(&expanded) {
                    Ok(bytes) => ReadOutcome::Present {
                        bytes,
                        mode: file_mode(&expanded),
                    },
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        ReadOutcome::Absent
                    }
                    Err(error) => ReadOutcome::Unreadable {
                        reason: error.to_string(),
                    },
                },
            };
        }
        match std::fs::read(&target) {
            Ok(bytes) => ReadOutcome::Present {
                bytes,
                mode: file_mode(&target),
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ReadOutcome::Absent,
            Err(error) => ReadOutcome::Unreadable {
                reason: error.to_string(),
            },
        }
    }

    fn snapshot_tree(&self, document: &ManifestDocument) -> BTreeMap<String, TreeMemberRead> {
        let dir = self.resolve(&document.destination);
        let mut out = BTreeMap::new();
        walk_tree(&dir, &dir, &mut out);
        out
    }

    fn write_documents(
        &self,
        documents: &[ManifestDocument],
        blobs: &dyn BlobStore,
        changed: &BTreeSet<Route>,
        on_written: Option<&dyn Fn(&Route)>,
    ) -> Result<usize> {
        let mut written = 0;
        for document in documents {
            let expanded = self.resolve(&document.destination);
            if document.data.unmanaged()
                && expanded.exists()
                && !changed.contains(&document.destination)
            {
                continue;
            }
            let outcome = match &document.data {
                ManifestData::Link { target } => {
                    write_link(&expanded, Path::new(target)).map_err(Error::from)
                }
                ManifestData::Tree { members } => write_tree_members(&expanded, members, blobs),
                ManifestData::Opaque { blob, .. } => {
                    if std::fs::read_link(&expanded).is_ok()
                        && let Err(error) = std::fs::remove_file(&expanded)
                    {
                        return Err(Error::Plan(format!(
                            "cannot remove link '{}': {error}",
                            expanded.display()
                        )));
                    }
                    copy_blob(blob, blobs, &expanded)
                }
                ManifestData::Secret { argv, .. } => {
                    let bytes = run_secret_command(argv, &document.destination, &|route| {
                        self.resolve(route)
                    })?;
                    write_file(&expanded, &bytes).map_err(Error::from)
                }
                _ => {
                    if std::fs::read_link(&expanded).is_ok()
                        && let Err(error) = std::fs::remove_file(&expanded)
                    {
                        return Err(Error::Plan(format!(
                            "cannot remove link '{}': {error}",
                            expanded.display()
                        )));
                    }
                    let bytes = document.render(&|route| self.resolve(route))?;
                    write_file(&expanded, &bytes).map_err(Error::from)
                }
            };
            if let Err(error) = outcome {
                return Err(Error::Plan(format!(
                    "cannot write '{}': {error}",
                    expanded.display()
                )));
            }
            if let Some(mode) = document.mode()
                && let Err(error) = set_mode(&expanded, mode)
            {
                return Err(Error::Plan(format!(
                    "cannot set mode '{}': {error}",
                    expanded.display()
                )));
            }
            if let Some(notify) = on_written {
                notify(&document.destination);
            }
            written += 1;
        }
        Ok(written)
    }

    fn remove_orphans(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize> {
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
            let expanded = self.resolve(&old.destination);
            if !expanded.exists() {
                continue;
            }
            std::fs::remove_file(&expanded).map_err(Error::from)?;
            removed += 1;
        }
        Ok(removed)
    }

    fn remove_tree_members(
        &self,
        recorded: &[ManifestDocument],
        desired: &[ManifestDocument],
    ) -> Result<usize> {
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
                let path = dest.join(&member.relative);
                if !path.exists() {
                    continue;
                }
                std::fs::remove_file(&path).map_err(Error::from)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    fn scaffold(&self, dest: &Route, name: &str) -> Result<()> {
        let dest = self.resolve(dest);
        let profile = dest.join(PROFILE_FILE);
        let stubs = dest.join(STUBS_DIR);
        if stubs.exists() {
            return Err(Error::Plan(format!(
                "init: '{}' already exists, remove it or pick another target",
                stubs.display()
            )));
        }
        if profile.exists() {
            return Err(Error::Plan(format!(
                "init: '{}' already exists, remove it or pick another target",
                profile.display()
            )));
        }
        std::fs::create_dir_all(&stubs).map_err(Error::from)?;
        write_file(&profile, scaffold_text(name).as_bytes()).map_err(Error::from)?;
        Ok(())
    }

    fn read_profile(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(handle.canonical())
    }

    fn read_module(&self, handle: &ResourceHandle) -> Result<String> {
        read_text(handle.canonical())
    }
}

/// Expands one destination route against host environment folders.
///
/// Home reads `HOME`. Config reads `XDG_CONFIG_HOME` else
/// home `.config`. Data reads `XDG_DATA_HOME` else home
/// `.local/share`. Cache reads `XDG_CACHE_HOME` else home
/// `.cache`. Literal carries its path verbatim. Unset homes
/// pass the relative path through intact.
fn resolve_host(route: &Route) -> PathBuf {
    let relative = route.relative();
    match route.base() {
        RouteBase::Literal => relative.to_path_buf(),
        RouteBase::Home => match home_dir() {
            Some(home) => home.join(relative),
            None => relative.to_path_buf(),
        },
        RouteBase::Config => match config_dir() {
            Some(base) => base.join(relative),
            None => relative.to_path_buf(),
        },
        RouteBase::Data => match data_dir() {
            Some(base) => base.join(relative),
            None => relative.to_path_buf(),
        },
        RouteBase::Cache => match cache_dir() {
            Some(base) => base.join(relative),
            None => relative.to_path_buf(),
        },
    }
}

/// Reads the home folder from the environment.
fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// Reads the config folder from the environment.
fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    home_dir().map(|home| home.join(".config"))
}

/// Reads the data folder from the environment.
fn data_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    home_dir().map(|home| home.join(".local/share"))
}

/// Reads the cache folder from the environment.
fn cache_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME").filter(|dir| !dir.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    home_dir().map(|home| home.join(".cache"))
}

/// Walks one folder into relative member reads.
///
/// Missing folders read empty.
fn walk_tree(root: &Path, dir: &Path, out: &mut BTreeMap<String, TreeMemberRead>) {
    let children = match std::fs::read_dir(dir) {
        Ok(children) => children,
        Err(_) => return,
    };
    for child in children {
        let child = match child {
            Ok(child) => child.path(),
            Err(_) => continue,
        };
        let rel = match child.strip_prefix(root) {
            Ok(rel) => rel.to_path_buf(),
            Err(_) => continue,
        };
        let Some(name) = rel.to_str() else {
            continue;
        };
        if let Ok(mut items) = std::fs::read_dir(&child)
            && items.next().is_some()
        {
            walk_tree(root, &child, out);
            continue;
        }
        if let Ok(target) = std::fs::read_link(&child) {
            out.insert(
                name.to_string(),
                TreeMemberRead::Present {
                    bytes: target.as_os_str().as_encoded_bytes().to_vec(),
                    mode: None,
                },
            );
            continue;
        }
        match std::fs::read(&child) {
            Ok(bytes) => {
                out.insert(
                    name.to_string(),
                    TreeMemberRead::Present {
                        bytes,
                        mode: file_mode(&child),
                    },
                );
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                out.insert(
                    name.to_string(),
                    TreeMemberRead::Unreadable {
                        reason: error.to_string(),
                    },
                );
            }
        }
    }
}

/// Joins a link target against the link parent without touching disk.
///
/// Relative targets resolve beside the link. Parent segments pop
/// lexically, staying put past the root.
fn join_link_target(link: &Path, target: &Path) -> PathBuf {
    if target.is_absolute() {
        return target.to_path_buf();
    }
    let mut out = match link.parent() {
        Some(parent) => parent.to_path_buf(),
        None => PathBuf::new(),
    };
    for part in target.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            _ => out.push(part),
        }
    }
    out
}

/// Reads unix permission bits for one path.
///
/// Links read as None.
fn file_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() {
        return None;
    }
    Some(metadata.permissions().mode() & 0o777)
}

/// Creates the parent folder for one destination path.
///
/// Empty parents skip.
///
/// # Errors
///
/// Missing ancestors and permission failures surface as io errors.
fn ensure_parent(dest: &Path) -> std::io::Result<()> {
    if let Some(parent) = dest.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Writes bytes to one path, creating parents as needed.
///
/// # Errors
///
/// Missing parents and permission failures surface as io errors.
fn write_file(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    ensure_parent(dest)?;
    std::fs::write(dest, bytes)
}

/// Creates one symlink, replacing present files.
///
/// Present files and links clear first, leaving their targets alone.
///
/// # Errors
///
/// Missing parents and permission failures surface as io errors.
fn write_link(link: &Path, target: &Path) -> std::io::Result<()> {
    ensure_parent(link)?;
    if std::fs::symlink_metadata(link).is_ok() {
        std::fs::remove_file(link)?;
    }
    std::os::unix::fs::symlink(target, link)
}

/// Sets unix permission bits on one path.
///
/// # Errors
///
/// Missing paths and permission failures surface as io errors.
fn set_mode(path: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Streams one blob to its destination through the blob pool.
///
/// # Errors
///
/// Dangling hashes fail as plan errors naming the hash.
/// Unreadable sources and unwritable destinations fail as
/// plan or io errors.
fn copy_blob(handle: &BlobHandle, blobs: &dyn BlobStore, dest: &Path) -> Result<()> {
    let mut reader = blobs.open(handle)?;
    ensure_parent(dest).map_err(Error::from)?;
    let mut out = std::fs::File::create(dest).map_err(Error::from)?;
    std::io::copy(&mut reader, &mut out).map_err(Error::from)?;
    out.flush().map_err(Error::from)?;
    Ok(())
}

/// Writes one tree destination member by member.
///
/// Only the members land, never the destination folder
/// itself. Parent folders create as needed, modes land
/// per member from the manifest. Member bytes stream from
/// the blob pool.
///
/// # Errors
///
/// Missing blobs, unwritable destinations and mode
/// failures surface as plan or io errors carrying the
/// member path.
fn write_tree_members(
    dest: &Path,
    members: &[ManifestMember],
    blobs: &dyn BlobStore,
) -> Result<()> {
    for member in members {
        let path = dest.join(&member.relative);
        copy_blob(&member.blob, blobs, &path).map_err(|error| match error {
            Error::Plan(_) => Error::Plan(format!(
                "missing blob '{}' for '{}'",
                member.blob.sha(),
                path.display()
            )),
            Error::Io(error) => Error::Io(std::io::Error::new(
                error.kind(),
                format!("cannot write '{}': {error}", path.display()),
            )),
        })?;
        set_mode(&path, member.mode).map_err(|error| {
            Error::Io(std::io::Error::new(
                error.kind(),
                format!("cannot set mode '{}': {error}", path.display()),
            ))
        })?;
    }
    Ok(())
}

/// Reads one file as text.
///
/// Missing files and invalid text fail as plan errors.
///
/// # Errors
///
/// Missing files fail as plan errors naming absence, invalid
/// text fails as plan errors, unreadable files surface as io
/// errors.
fn read_text(path: &Path) -> Result<String> {
    match std::fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| Error::Plan(format!("cannot read '{}': invalid text", path.display()))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(Error::Plan(format!(
            "cannot read '{}': missing file",
            path.display()
        ))),
        Err(error) => Err(Error::from(error)),
    }
}

/// Renders one starter profile naming the project.
///
/// The profile holds comments alone, so it parses anywhere.
fn scaffold_text(name: &str) -> String {
    format!("-- {name} profile.\n-- Add configs and documents here.\n")
}
