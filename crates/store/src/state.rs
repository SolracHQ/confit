//! Stored state manifests with picker resolution.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use confit_core::{
    error::{
        ContextualCause, ContextualResult, ErrorCause, FilesystemError, ManifestError, StateReason,
        Terminal, TerminalResult, WithContext, contextual, error, kind::ErrorKind,
    },
    resource_ref::ResourceRef,
    sha::Sha,
    version::VERSION,
};
use confit_differ::view::View;
use confit_driver as driver;
use confit_events::EventHandles;
use confit_events::protocol::{TaskKind, TaskNote};
use confit_model::{
    document::{Data, Document, OpaqueDocument},
    manifest::Manifest,
};

use crate::{StoreRoots, resources::Resources};

/// The number of history files kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 6;

/// The folder name for history entries.
const HISTORY_DIR: &str = "history";

/// The folder name for named states.
const STATES_DIR: &str = "states";

/// The named state that holds the pre-execution rescue.
const RESCUE_NAME: &str = "!";

/// The path of one stored state file.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateHandle {
    path: PathBuf,
}

impl StateHandle {
    /// Returns the path of the state file.
    pub fn canonical(&self) -> &Path {
        &self.path
    }

    /// Builds a handle for one state file path.
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

/// A picker for one stored state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picker {
    /// A state selected by its stored name.
    Named(String),
    /// A history entry.
    History(usize),
}

impl FromStr for Picker {
    type Err = ContextualCause;

    fn from_str(raw: &str) -> ContextualResult<Self> {
        if let Some(name) = raw.strip_prefix('@') {
            validate_state_name(name)?;
            return Ok(Self::Named(name.to_owned()));
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = match rest.parse() {
                Ok(pick) => pick,
                Err(_) => {
                    return Err(contextual(StateReason::NotNumber));
                }
            };
            if pick == 0 {
                return Err(contextual(StateReason::OutOfRange));
            }
            return Ok(Self::History(pick));
        }
        Err(contextual(StateReason::BareState))
    }
}

/// A store of history entries and named state files with picker choices.
#[derive(Debug, Clone)]
pub struct StateStore {
    history: PathBuf,
    states: PathBuf,
    resources: Arc<Resources>,
    handles: EventHandles,
}

impl StateStore {
    /// Builds a state store under the config base.
    pub fn new(roots: &StoreRoots, resources: Arc<Resources>, handles: EventHandles) -> Self {
        Self {
            history: roots.config_base.join(HISTORY_DIR),
            states: roots.config_base.join(STATES_DIR),
            resources,
            handles,
        }
    }

    /// Loads the applied manifest, or an empty manifest when history is absent.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::CorruptManifest`] when a state is unreadable or the payload is bad.
    /// - [`ErrorKind::ManifestVersion`] when the version is unsupported.
    /// - [`ErrorKind::ManifestNoVersion`] when the version is absent.
    pub fn load_applied(&self) -> TerminalResult<Manifest> {
        match self.applied() {
            Ok(handle) => self.load_manifest(&handle),
            Err(error) => match error.cause() {
                ErrorCause::StateReason(_)
                | ErrorCause::FilesystemError(FilesystemError::FileMissing) => Ok(empty()),
                _ => Err(error.terminal(ErrorKind::CorruptManifest)),
            },
        }
    }

    /// Saves one manifest into a new history file.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::StoreState`] when the manifest render fails.
    /// - [`ErrorKind::RotateHistory`] when the history rotation fails.
    pub fn save_applied(&self, manifest: &Manifest) -> TerminalResult<()> {
        let text = manifest.json().terminal(ErrorKind::StoreState)?;
        let mut bytes = text.as_bytes();
        let mut stamp = system_nanos().terminal(ErrorKind::RotateHistory)?;
        let mut destination = self.history.join(format!("{stamp}.json"));
        while destination.exists() {
            stamp += 1;
            destination = self.history.join(format!("{stamp}.json"));
        }
        store_bytes(&destination, &mut bytes).terminal(ErrorKind::RotateHistory)?;
        checked(destination).terminal(ErrorKind::RotateHistory)?;
        let files = history_files(&self.history).terminal(ErrorKind::RotateHistory)?;
        if files.len() > HISTORY_KEPT {
            for stale in files.iter().take(files.len() - HISTORY_KEPT) {
                std::fs::remove_file(stale)
                    .map_err(ContextualCause::from)
                    .with_fs_path(stale)
                    .terminal(ErrorKind::RotateHistory)?;
            }
        }
        Ok(())
    }

    /// Loads the manifest that one picker holds.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::CorruptManifest`] when a state is absent or unreadable, or the payload is
    ///   bad.
    /// - [`ErrorKind::ManifestVersion`] when the version is unsupported.
    /// - [`ErrorKind::ManifestNoVersion`] when the version is absent.
    pub fn load_state(&self, picker: &Picker) -> TerminalResult<Manifest> {
        let handle = self.resolve(picker).terminal(ErrorKind::CorruptManifest)?;
        self.load_manifest(&handle)
    }

    /// Saves one manifest under one state name.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::StoreState`] when the name is bad, the render fails,
    ///   or the write fails.
    pub fn save_named(&self, name: &str, manifest: &Manifest) -> TerminalResult<()> {
        let pack_path = PathBuf::from(format!("@{name}"));
        let total = manifest.documents.len() + manifest.hooks.len();
        let reporter = self.handles.start_task(
            TaskKind::Pack {
                path: pack_path.clone(),
            },
            Some(total),
        );
        let path = self.named_path(name).terminal(ErrorKind::StoreState)?;
        let text = manifest.json().terminal(ErrorKind::StoreState)?;
        let mut bytes = text.as_bytes();
        store_bytes(&path, &mut bytes).terminal(ErrorKind::StoreState)?;
        checked(path).terminal(ErrorKind::StoreState)?;
        reporter.progress(total, Some(total), None);
        reporter.finish(TaskNote::Members(total));
        Ok(())
    }

    /// Saves the before hashes of the effective rows under the rescue name.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::StoreState`] when the disk capture, pool persist,
    ///   or rescue save fails.
    pub fn save_rescue(&self, view: &View) -> TerminalResult<usize> {
        if view.iter().all(|change| change.before.is_none()) {
            return Ok(0);
        }
        let mut documents = Vec::new();
        for change in view.iter() {
            let Some(before) = change.before else {
                continue;
            };
            self.resources
                .unchecked(&before.sha, change.expanded.clone());
            documents.push(Document::new(
                change.path.clone(),
                Data::Opaque(OpaqueDocument {
                    handle: ResourceRef::new(before.sha),
                    mode: before.mode,
                    unmanaged: false,
                }),
            ));
        }
        let manifest = Manifest {
            version: VERSION,
            documents,
            hooks: Vec::new(),
        };
        self.resources
            .persist(&manifest.handles())
            .terminal(ErrorKind::StoreState)?;
        self.save_named(RESCUE_NAME, &manifest)?;
        Ok(manifest.documents.len())
    }

    /// Removes the state file that one named picker holds.
    ///
    /// # Errors
    ///
    /// - [`StateReason::BareState`] when the picker is not a named state.
    /// - [`FilesystemError`] when the state file fails to remove.
    pub fn remove_state(&self, picker: &Picker) -> ContextualResult<()> {
        let Picker::Named(_) = picker else {
            return Err(contextual(StateReason::BareState));
        };
        let handle = self.resolve(picker)?;
        let path = handle.canonical();
        std::fs::remove_file(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)
    }

    /// Resolves one picker into its state handle.
    ///
    /// # Errors
    ///
    /// - [`StateReason::MissingEntry`] when the state file is absent.
    /// - [`StateReason::OutOfRange`] when the pick is out of range.
    /// - [`FilesystemError`] when the history folder is unreadable.
    pub fn resolve(&self, picker: &Picker) -> ContextualResult<StateHandle> {
        match picker {
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                checked(path)
            }
            Picker::History(pick) => {
                let files = history_files(&self.history)?;
                if *pick >= files.len() {
                    return Err(contextual(StateReason::OutOfRange).with_fs_path(&self.history));
                }
                let Some(path) = files.into_iter().rev().nth(*pick) else {
                    return Err(contextual(StateReason::OutOfRange).with_fs_path(&self.history));
                };
                Ok(StateHandle::new(path))
            }
        }
    }

    /// Returns the history handles.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the history folder is unreadable.
    pub fn history_list(&self) -> ContextualResult<Vec<StateHandle>> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        Ok(files.into_iter().map(StateHandle::new).collect())
    }

    /// Returns whether the history is empty.
    pub fn is_first_execution(&self) -> bool {
        history_files(&self.history)
            .map(|files| files.is_empty())
            .unwrap_or(true)
    }

    /// Collects the content hashes that the states hold for a sweep.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when a folder is unreadable.
    pub(crate) fn keep_set(&self) -> ContextualResult<BTreeSet<Sha>> {
        let mut keep = BTreeSet::new();
        if let Ok(handle) = self.applied() {
            self.collect_state_refs(&handle, &mut keep);
        }
        for handle in self.history_list()? {
            self.collect_state_refs(&handle, &mut keep);
        }
        for handle in self.list_named()? {
            self.collect_state_refs(&handle, &mut keep);
        }
        Ok(keep)
    }

    /// Loads one state file with version errors mapped to terminal kinds.
    ///
    /// # Errors
    ///
    /// - [`ErrorKind::CorruptManifest`] when a state is unreadable or the payload is bad.
    /// - [`ErrorKind::ManifestVersion`] when the version is unsupported.
    /// - [`ErrorKind::ManifestNoVersion`] when the version is absent.
    fn load_manifest(&self, handle: &StateHandle) -> TerminalResult<Manifest> {
        match self.load_handle(handle) {
            Ok(found) => Ok(found),
            Err(contextual) => {
                let kind = match contextual.cause() {
                    ErrorCause::ManifestError(ManifestError::VersionUnsupported { .. }) => {
                        ErrorKind::ManifestVersion
                    }
                    ErrorCause::ManifestError(ManifestError::MissingVersion) => {
                        ErrorKind::ManifestNoVersion
                    }
                    _ => ErrorKind::CorruptManifest,
                };
                error(kind, contextual)
            }
        }
    }

    /// Reads one state file and decodes its manifest.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state file is unreadable.
    /// - [`ManifestError`] when the payload is bad.
    fn load_handle(&self, handle: &StateHandle) -> ContextualResult<Manifest> {
        let bytes = self.read(handle)?;
        Manifest::decode(&bytes).with_fs_path(handle.canonical())
    }

    /// Collects the content hashes from one state manifest that decodes.
    fn collect_state_refs(&self, handle: &StateHandle, keep: &mut BTreeSet<Sha>) {
        let found: Manifest = match self.load_handle(handle) {
            Ok(found) => found,
            Err(_) => return,
        };
        for handle in found.handles() {
            keep.insert(*handle.sha());
        }
    }

    /// Returns the handle for the applied state.
    ///
    /// # Errors
    ///
    /// - [`StateReason::MissingEntry`] when the history is empty.
    fn applied(&self) -> ContextualResult<StateHandle> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        files
            .into_iter()
            .next()
            .map(StateHandle::new)
            .ok_or_else(|| contextual(StateReason::MissingEntry).with_fs_path(&self.history))
    }

    /// Reads one state file.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state file is unreadable.
    fn read(&self, handle: &StateHandle) -> ContextualResult<Vec<u8>> {
        let path = handle.canonical();
        std::fs::read(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)
    }

    /// Lists named state handles.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state folder is unreadable.
    fn list_named(&self) -> ContextualResult<Vec<StateHandle>> {
        Ok(history_files(&self.states)?
            .into_iter()
            .map(StateHandle::new)
            .collect())
    }

    /// Returns the file path for one validated state name.
    ///
    /// # Errors
    ///
    /// - [`StateReason::EmptyName`] when the name is empty.
    /// - [`StateReason::ForbiddenNameChars`] when the name holds a forbidden character.
    fn named_path(&self, name: &str) -> ContextualResult<PathBuf> {
        validate_state_name(name)?;
        Ok(self.states.join(format!("{name}.json")))
    }
}

/// Builds one manifest with no documents at the current version.
fn empty() -> Manifest {
    Manifest {
        version: VERSION,
        documents: Vec::new(),
        hooks: Vec::new(),
    }
}

/// Returns the handle for one state file that exists.
///
/// # Errors
///
/// - [`StateReason::MissingEntry`] when the state file is absent.
fn checked(path: PathBuf) -> ContextualResult<StateHandle> {
    if !path.exists() {
        return Err(contextual(StateReason::MissingEntry).with_fs_path(path));
    }
    Ok(StateHandle::new(path))
}

/// Returns the stored files.
///
/// # Errors
///
/// - [`FilesystemError`] when the folder is unreadable.
fn history_files(directory: &Path) -> ContextualResult<Vec<PathBuf>> {
    match driver::fs::read_dir(directory) {
        Ok(files) => Ok(files),
        Err(error)
            if matches!(
                error.cause(),
                ErrorCause::FilesystemError(FilesystemError::FileMissing)
            ) =>
        {
            Ok(Vec::new())
        }
        Err(error) => Err(error),
    }
}

/// Returns the clock reading in nanoseconds for history file names.
///
/// # Errors
///
/// - [`FilesystemError::FileUnknown`] when the clock is before the epoch.
fn system_nanos() -> ContextualResult<u128> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_nanos())
        .map_err(ContextualCause::from)
}

/// Validates one state name as a file name.
///
/// # Errors
///
/// - [`StateReason::EmptyName`] when the name is empty.
/// - [`StateReason::ForbiddenNameChars`] when the name holds a forbidden character.
fn validate_state_name(name: &str) -> ContextualResult<()> {
    if name.is_empty() {
        return Err(contextual(StateReason::EmptyName));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(contextual(StateReason::ForbiddenNameChars));
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(contextual(StateReason::ForbiddenNameChars));
    }
    if Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(contextual(StateReason::ForbiddenNameChars));
    }
    Ok(())
}

/// Writes one streamed source into a state file.
///
/// # Errors
///
/// - [`FilesystemError`] when the state file fails to write.
fn store_bytes(path: &Path, source: &mut impl Read) -> ContextualResult<()> {
    driver::StagedFile::write(path, |file| {
        std::io::copy(&mut *source, file)?;
        file.sync_all()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Stores;
    use confit_differ::{
        change::{Change, ChangeKind, Comparability, IncomparableReason, Side},
        diff::Diff,
        view::{View, ViewKind},
    };
    use confit_events::EventHandles;
    use confit_model::{
        document::FileMode,
        routes::{Route, RouteBase},
    };

    /// Builds silent stores under one temporary folder.
    fn test_stores(dir: &Path) -> Stores {
        Stores::new(StoreRoots::under(dir, dir), EventHandles::silent())
    }

    /// Builds one literal route under one temporary folder.
    fn test_route(dir: &Path, name: &str) -> Route {
        Route::new(RouteBase::Literal, dir.join(name)).unwrap()
    }

    /// Builds one effective row with the before hash.
    fn test_change(path: Route, before: Option<Sha>) -> Change {
        let expanded = path.resolve().unwrap();
        Change {
            path,
            expanded,
            kind: ChangeKind::Change,
            comparability: Comparability::Comparable,
            group: None,
            before: before.map(|hash| Side::new(hash, FileMode::default())),
            after: None,
        }
    }

    /// Builds a diff that holds the effective rows.
    fn test_diff(changes: Vec<Change>) -> Diff {
        let wanted = Arc::new(Manifest {
            version: VERSION,
            documents: Vec::new(),
            hooks: Vec::new(),
        });
        let recorded = Arc::new(Manifest {
            version: VERSION,
            documents: Vec::new(),
            hooks: Vec::new(),
        });
        Diff {
            wanted: Arc::clone(&wanted),
            recorded: Arc::clone(&recorded),
            desired: View::new(
                ViewKind::Desired,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
            drift: View::new(
                ViewKind::Drift,
                Vec::new(),
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
            effective: View::new(
                ViewKind::Effective,
                changes,
                Arc::clone(&wanted),
                Arc::clone(&recorded),
            ),
        }
    }

    /// Loads the rescue manifest that the save wrote.
    fn rescue_manifest(stores: &Stores) -> Manifest {
        let picker = Picker::Named(RESCUE_NAME.to_owned());
        match stores.states().load_state(&picker) {
            Ok(found) => found,
            Err(error) => panic!("the rescue state fails to load: {error}"),
        }
    }

    #[test]
    fn rescue_skips_the_save_without_before_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let diff = test_diff(vec![test_change(test_route(dir.path(), "fresh"), None)]);
        match stores.states().save_rescue(&diff.effective) {
            Ok(saved) => assert_eq!(saved, 0, "the addition saves nothing"),
            Err(error) => panic!("the rescue save fails: {error}"),
        }
        let picker = Picker::Named(RESCUE_NAME.to_owned());
        assert!(
            stores.states().resolve(&picker).is_err(),
            "the skip leaves the rescue state absent"
        );
    }

    #[test]
    fn rescue_saves_the_before_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        std::fs::write(dir.path().join("note"), b"original").unwrap();
        let before = Sha::hash(b"original");
        let route = test_route(dir.path(), "note");
        let diff = test_diff(vec![test_change(route.clone(), Some(before))]);
        match stores.states().save_rescue(&diff.effective) {
            Ok(saved) => assert_eq!(saved, 1, "the rescue saves one file"),
            Err(error) => panic!("the rescue save fails: {error}"),
        }
        let found = rescue_manifest(&stores);
        assert_eq!(found.documents.len(), 1, "the rescue holds one document");
        assert_eq!(found.documents[0].path, route, "the rescue keeps the route");
        assert!(
            stores.pool().path(&before).exists(),
            "the rescue pools the before bytes"
        );
    }

    #[test]
    fn rescue_skips_rows_without_before_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        std::fs::write(dir.path().join("kept"), b"kept").unwrap();
        let before = Sha::hash(b"kept");
        let rugged = Change {
            kind: ChangeKind::Change,
            comparability: Comparability::Incomparable(IncomparableReason::DirThere),
            ..test_change(test_route(dir.path(), "rugged"), None)
        };
        let diff = test_diff(vec![
            rugged,
            test_change(test_route(dir.path(), "kept"), Some(before)),
        ]);
        match stores.states().save_rescue(&diff.effective) {
            Ok(saved) => assert_eq!(saved, 1, "the rescue saves the lone hash"),
            Err(error) => panic!("the rescue save fails: {error}"),
        }
        assert_eq!(
            rescue_manifest(&stores).documents.len(),
            1,
            "the rescue drops the hash-free row"
        );
    }

    #[test]
    fn rescue_reuses_pooled_bytes_without_disk_reads() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        let before = Sha::hash(b"pooled");
        match stores.pool().store_checked(&before, &b"pooled"[..]) {
            Ok(()) => {}
            Err(error) => panic!("the pool fails to store: {error}"),
        }
        let diff = test_diff(vec![test_change(
            test_route(dir.path(), "absent"),
            Some(before),
        )]);
        match stores.states().save_rescue(&diff.effective) {
            Ok(saved) => assert_eq!(saved, 1, "the pooled hash saves one file"),
            Err(error) => panic!("the rescue save fails: {error}"),
        }
    }

    #[test]
    fn rescue_rejects_mismatched_disk_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let stores = test_stores(dir.path());
        std::fs::write(dir.path().join("note"), b"disk").unwrap();
        let diff = test_diff(vec![test_change(
            test_route(dir.path(), "note"),
            Some(Sha::hash(b"other")),
        )]);
        assert!(
            stores.states().save_rescue(&diff.effective).is_err(),
            "the mismatch fails the rescue save"
        );
    }
}
