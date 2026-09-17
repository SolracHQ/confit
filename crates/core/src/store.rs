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
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::{PLAN_VERSION, Plan};
/// use confit_core::store::write_plan;
///
/// let plan = Plan { version: PLAN_VERSION, documents: Vec::new(), created_at: String::new(), hooks: Vec::new() };
/// assert!(matches!(write_plan(&plan, None, &MemoryFs::new()), Ok(())));
/// ```
/// Serializes one plan with per-document parallelism.
///
/// Documents serialize independently across rayon threads,
/// then join in path order. Hooks serialize sequentially.
/// Output bytes match sequential serde exactly, keeping
/// every reader unchanged.
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
    let hooks = serde_json::to_string(&plan.hooks)
        .map_err(|error| Error::Plan(format!("render plan: {error}")))?;
    Ok(format!(
        "{{\"version\":{},\"documents\":[{}],\"created_at\":{},\"hooks\":{}}}",
        plan.version,
        bodies.join(","),
        created,
        hooks
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
            DocumentData::Tree { members } => write_tree_members(&expanded, members, fs),
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
/// ```text
/// use confit_core::document::{Document, DocumentData, TreeMember};
/// use confit_core::fs::MemoryFs;
/// use confit_core::ids::DocPath;
/// use confit_core::store::remove_tree_members;
///
/// let fs = MemoryFs::new();
/// let recorded = vec![Document::new(
///     DocPath::new("fonts"),
///     DocumentData::Tree { members: vec![TreeMember { rel: "gone.ttf".into(), content: vec![1], mode: 0o644 }] },
/// }];
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
/// ```text
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
/// ```text
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
/// ```text
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
        let sequential = match serde_json::to_string(&plan) {
            Ok(text) => text,
            Err(error) => panic!("sequential serializes: {error}"),
        };
        assert_eq!(parallel, sequential);
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
}
