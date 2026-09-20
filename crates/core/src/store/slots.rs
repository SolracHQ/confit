//! Slots
//!
//! State slot plus history rotation plus dir resolution.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::fs::Filesystem;
use crate::plan::{BUNDLE_VERSION, Bundle};
use crate::progress::ProgressSender;

use super::blobs::{Hydrator, store_blobs};
use super::manifest::{HistoryEntry, Manifest, manifest_json};

/// Loads previous manifest, treating missing files as empty.
///
/// # Arguments
///
/// * `path` - the state file, holding `None` for empty previous.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The parsed manifest, else empty for missing inputs.
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
/// use confit_core::plan::BUNDLE_VERSION;
/// use confit_core::store::slots::load_state;
///
/// let outcome = load_state(None, &MemoryFs::new());
/// assert!(matches!(outcome, Ok(bundle) if bundle.manifest.documents.is_empty() && bundle.manifest.version == BUNDLE_VERSION));
/// ```
pub fn load_state(path: Option<&Path>, fs: &dyn Filesystem) -> Result<Bundle> {
    let Some(file) = path else {
        return Ok(Bundle::empty());
    };
    let bytes = match fs.read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Bundle::empty());
        }
        Err(error) => return Err(Error::from(error)),
    };
    let read_start = std::time::Instant::now();
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", file.display())))?;
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
                file.display()
            )));
        }
    }
    let stored: Manifest = serde_json::from_value(value)
        .map_err(|error| Error::Plan(format!("read state '{}': {error}", file.display())))?;
    let bundle = Hydrator::new(file, fs)?.hydrate(&stored)?;
    log::debug!(
        "read took {}ms for {}",
        read_start.elapsed().as_millis(),
        file.display()
    );
    Ok(bundle)
}

/// Writes the manifest payload to a file or stdout.
///
/// Manifest writes store missing pool blobs first, so the
/// file stays resolvable after the write. Both destinations
/// take the pretty manifest form.
///
/// # Arguments
///
/// * `bundle` - the versioned desired state.
/// * `out` - the destination, holding `None` for stdout.
/// * `fs` - the backend under writing.
/// * `progress` - the sink for compression facts, holding `None` for silence.
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
/// use confit_core::plan::Bundle;
/// use confit_core::store::slots::write_manifest;
///
/// let bundle = Bundle::empty();
/// assert!(matches!(write_manifest(&bundle, None, &MemoryFs::new(), None), Ok(())));
/// ```
pub fn write_manifest(
    bundle: &Bundle,
    out: Option<&Path>,
    fs: &dyn Filesystem,
    progress: Option<&ProgressSender>,
) -> Result<()> {
    store_blobs(bundle, fs, progress)?;
    let text = manifest_json(bundle)?;
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

/// Stored plans kept before rotation drops the oldest.
const PREVIOUS_KEPT: usize = 5;

/// Lists stored manifests newest first with apply picks.
///
/// # Arguments
///
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// Stored entries with apply picks per index.
///
/// # Errors
///
/// Folder resolution failures surface as plan errors.
///
pub fn list_previous(fs: &dyn Filesystem) -> Result<Vec<HistoryEntry>> {
    let dir = resolve_previous_dir()?;
    Ok(stored_entries(&dir, fs)?
        .into_iter()
        .enumerate()
        .map(|(position, _)| HistoryEntry {
            index: position + 1,
        })
        .collect())
}

/// Reads stored manifests newest first with their file paths.
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
/// Stored manifests with file paths in sorted order.
///
/// # Errors
///
/// Folder listing failures beyond missing folders surface
/// as io errors.
///
pub fn stored_entries(dir: &Path, fs: &dyn Filesystem) -> Result<Vec<(PathBuf, Bundle)>> {
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
        if stored.version != BUNDLE_VERSION {
            continue;
        }
        let mut hydrator = match Hydrator::new(&file, fs) {
            Ok(hydrator) => hydrator,
            Err(_) => continue,
        };
        let Ok(bundle) = hydrator.hydrate(&stored) else {
            continue;
        };
        out.push((file, bundle));
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

/// Drops stored manifests past the kept count, oldest first.
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
/// use confit_core::store::slots::resolve_base_dir;
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
/// The folder holding stored manifests.
///
/// # Errors
///
/// Missing OS config folders fail as plan errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::store::slots::resolve_previous_dir;
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
/// use confit_core::store::slots::resolve_plans_dir;
///
/// let dir = resolve_plans_dir();
/// assert!(matches!(dir, Ok(dir) if dir.ends_with("confit/plans")));
/// ```
pub fn resolve_plans_dir() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("plans"))
}

/// Resolves one named slot file under the plans folder.
///
/// Names hold one file stem with no separators. Empty names,
/// separator carriers, plus dot segments fail as plan errors.
///
/// # Arguments
///
/// * `name` - the slot name without the `@` sigil.
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
/// use confit_core::store::slots::resolve_named_slot;
///
/// let path = resolve_named_slot("work");
/// assert!(matches!(path, Ok(path) if path.ends_with("confit/plans/work.json")));
/// ```
pub fn resolve_named_slot(name: &str) -> Result<PathBuf> {
    if name.is_empty() {
        return Err(Error::Plan("slot name reads empty".to_string()));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(Error::Plan(format!("slot name '{name}' holds separators")));
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(Error::Plan(format!("slot name '{name}' reads unsupported")));
    }
    if std::path::Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(Error::Plan(format!("slot name '{name}' reads unsupported")));
    }
    Ok(resolve_plans_dir()?.join(format!("{name}.json")))
}

/// Builds the fixed live state slot.
///
/// The slot holds the last applied manifest. History files
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
/// use confit_core::store::slots::default_state_path;
///
/// let slot = default_state_path();
/// assert!(matches!(slot, Ok(slot) if slot.ends_with("confit/state.json")));
/// ```
pub fn default_state_path() -> Result<PathBuf> {
    Ok(resolve_base_dir()?.join("state.json"))
}

/// Stores one applied manifest, rotating past the kept count.
///
/// Stamp names sort oldest first. Colliding stamps bump up
/// by one until the name reads fresh. Referenced blobs land
/// in the pool first, so the manifest stays resolvable.
///
/// # Arguments
///
/// * `bundle` - the applied manifest under storing.
/// * `fs` - the backend under writing.
/// * `progress` - the sink for compression facts, holding `None` for silence.
///
/// # Returns
///
/// The stored manifest path backing apply of the past.
///
/// # Errors
///
/// Clock plus write failures surface as plan or io errors.
///
/// # Examples
///
/// ```rust
/// use confit_core::fs::MemoryFs;
/// use confit_core::plan::Bundle;
/// use confit_core::store::slots::archive_previous;
///
/// let outcome = archive_previous(&Bundle::empty(), &MemoryFs::new(), None);
/// assert!(matches!(outcome, Ok(_)));
/// ```
pub fn archive_previous(
    bundle: &Bundle,
    fs: &dyn Filesystem,
    progress: Option<&ProgressSender>,
) -> Result<PathBuf> {
    store_blobs(bundle, fs, progress)?;
    let dir = resolve_previous_dir()?;
    let mut stamp = system_nanos()?;
    let mut dest = dir.join(format!("{stamp}.json"));
    while fs.exists(&dest) {
        stamp += 1;
        dest = dir.join(format!("{stamp}.json"));
    }
    let text = manifest_json(bundle)?;
    fs.write(&dest, text.as_bytes()).map_err(Error::from)?;
    rotate_previous(&dir, fs)?;
    Ok(dest)
}

/// One resolved slot picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    /// Holds the applied slot.
    Applied,
    /// Holds one named slot without the `@` sigil.
    Named(String),
    /// Holds one history pick newest-first from one.
    History(usize),
}

/// Resolves one picker to its live bundle plus slot kind.
///
/// Absent pickers read the applied slot. `@name` reads the
/// named slot. `%N` reads history newest-first from one.
/// Named plus applied slots refuse while their files read
/// absent. History refuses while the pick falls outside the
/// listing. Bare values refuse, so paths never parse as slots.
/// Callers prefix errors with their command name.
///
/// # Arguments
///
/// * `picker` - the raw picker value under resolving.
/// * `fs` - the backend under reading.
///
/// # Returns
///
/// The live bundle holding binary bytes, plus the slot kind.
///
/// # Errors
///
/// Absent slots plus malformed plus out-of-range picks plus
/// load failures surface as plan or io errors.
pub fn resolve_slot(picker: Option<&str>, fs: &dyn Filesystem) -> Result<(Bundle, SlotKind)> {
    let Some(raw) = picker else {
        let slot = default_state_path()?;
        if !fs.exists(&slot) {
            return Err(Error::Plan(
                "the applied slot reads absent, apply first".to_string(),
            ));
        }
        let bundle = load_state(Some(slot.as_path()), fs)?;
        return Ok((bundle, SlotKind::Applied));
    };
    if let Some(name) = raw.strip_prefix('@') {
        let path = resolve_named_slot(name)?;
        if !fs.exists(&path) {
            return Err(Error::Plan(format!("'@{name}' reads absent")));
        }
        let bundle = load_state(Some(path.as_path()), fs)?;
        return Ok((bundle, SlotKind::Named(name.to_string())));
    }
    if let Some(rest) = raw.strip_prefix('%') {
        let pick: usize = rest.parse().map_err(|_| {
            Error::Plan(format!(
                "'{raw}' reads unsupported, want '%N' holding a number from 1"
            ))
        })?;
        let dir = resolve_previous_dir()?;
        let entries = stored_entries(&dir, fs)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::Bundle;

    #[test]
    fn named_slot_resolves_under_plans_dir() {
        let path = match resolve_named_slot("work") {
            Ok(path) => path,
            Err(error) => panic!("named slot resolves: {error}"),
        };
        assert!(path.ends_with("confit/plans/work.json"));
    }

    #[test]
    fn named_plan_rejects_empty_separators_and_parent() {
        for name in ["", "a/b", "a\\b", ".", ".."] {
            match resolve_named_slot(name) {
                Ok(_) => panic!("{name:?} passes"),
                Err(error) => assert!(!error.to_string().is_empty()),
            }
        }
    }

    #[test]
    fn named_plan_roundtrips_through_memory_fs() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let built = match Bundle::build(
            vec![crate::document::ManifestDocument::new(
                crate::ids::DocPath::new("note"),
                crate::document::ManifestData::Text {
                    content: "hi".to_string(),
                    mode: None,
                    unmanaged: false,
                },
            )],
            Vec::new(),
        ) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let dest = match resolve_named_slot("work") {
            Ok(dest) => dest,
            Err(error) => panic!("named slot resolves: {error}"),
        };
        match write_manifest(&built, Some(&dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("named slot writes: {error}"),
        }
        let loaded = match load_state(Some(&dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("named slot loads: {error}"),
        };
        assert_eq!(loaded.manifest.documents.len(), 1);
    }

    #[test]
    fn hooks_roundtrip_through_memory_fs() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let hooks = vec![crate::hook::Hook {
            argv: vec!["mise".to_string(), "install".to_string()],
            path: vec!["/home/tester/.local/bin".to_string()],
            requires: None,
            when: Some(crate::condition::Condition::InPath {
                name: "mise".to_string(),
            }),
            checks: vec![crate::condition::Condition::InPath {
                name: "bat".to_string(),
            }],
            timeout_secs: crate::runtime::DEFAULT_HOOK_TIMEOUT_SECS,
        }];
        let built = match Bundle::build(Vec::new(), hooks) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let dest = std::path::Path::new("slot.json");
        match write_manifest(&built, Some(dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("manifest writes: {error}"),
        }
        let loaded = match load_state(Some(dest), &fs) {
            Ok(loaded) => loaded,
            Err(error) => panic!("manifest loads: {error}"),
        };
        assert_eq!(loaded, built);
        assert_eq!(loaded.manifest.hooks.len(), 1);
    }

    #[test]
    fn version_two_state_fails_as_unsupported() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        match fs.write(
            std::path::Path::new("state.json"),
            b"{\"version\":2,\"documents\":[],\"hooks\":[]}",
        ) {
            Ok(()) => {}
            Err(error) => panic!("memory writes: {error}"),
        }
        match load_state(Some(std::path::Path::new("state.json")), &fs) {
            Ok(_) => panic!("stale version passes"),
            Err(error) => assert_eq!(
                error.to_string(),
                format!("state version 2 reads unsupported, want {BUNDLE_VERSION}")
            ),
        }
    }

    #[test]
    fn resolve_slot_absent_applied_refuses() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        match resolve_slot(None, &fs) {
            Ok(_) => panic!("absent applied slot passes"),
            Err(error) => assert!(error.to_string().contains("reads absent")),
        }
    }

    #[test]
    fn resolve_slot_named_roundtrips_with_kind() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        let built = match Bundle::build(Vec::new(), Vec::new()) {
            Ok(built) => built,
            Err(error) => panic!("bundle builds: {error}"),
        };
        let dest = match resolve_named_slot("work") {
            Ok(dest) => dest,
            Err(error) => panic!("named slot resolves: {error}"),
        };
        match write_manifest(&built, Some(&dest), &fs, None) {
            Ok(()) => {}
            Err(error) => panic!("named slot writes: {error}"),
        }
        match resolve_slot(Some("@work"), &fs) {
            Ok((bundle, kind)) => {
                assert_eq!(bundle.manifest.documents.len(), 0);
                assert_eq!(kind, SlotKind::Named("work".to_string()));
            }
            Err(error) => panic!("named slot resolves: {error}"),
        }
    }

    #[test]
    fn resolve_slot_empty_history_refuses_pick() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        match resolve_slot(Some("%1"), &fs) {
            Ok(_) => panic!("empty history passes"),
            Err(error) => assert!(error.to_string().contains("out of range")),
        }
    }

    #[test]
    fn resolve_slot_bare_value_refuses() {
        use crate::fs::MemoryFs;

        let fs = MemoryFs::new();
        match resolve_slot(Some("backup.cb"), &fs) {
            Ok(_) => panic!("bare value passes"),
            Err(error) => assert!(error.to_string().contains("reads unsupported")),
        }
    }
}
