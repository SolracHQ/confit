//! Slot
//!
//! Applied state slot, named slots, and manifest history.

pub mod error;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::bundle::{BUNDLE_VERSION, Bundle};
use confit_model::document::BlobRef;
use confit_model::manifest::{Manifest, manifest_json};
use confit_model::progress::ProgressSender;

use crate::StoreRoots;
use confit_driver as driver;
use error::{Result, SlotError};

/// Stored plans kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 5;

/// State file name under the config base.
const STATE_FILE: &str = "state.json";

/// History folder name under the config base.
const PREVIOUS_DIR: &str = "previous";

/// Named slot folder name under the config base.
const PLANS_DIR: &str = "plans";

/// One slot kind selecting applied, named, or history bundles.
///
/// Applied holds the fixed state slot. Named holds one
/// `@name` slot without the sigil. History holds one `%N`
/// pick newest-first from one.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlotKind {
    /// Holds the applied slot.
    Applied,
    /// Holds one named slot without the `@` sigil.
    Named(String),
    /// Holds one history pick newest-first from one.
    History(usize),
}

/// One stored manifest entry for the apply-past listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// Holds the listing position used as the apply `%N` pick.
    pub index: usize,
}

/// Applied state slot with named slots and history.
///
/// History lists newest first with `%N` picks from one.
/// Applied, named, and history bundles ride config base files.
#[derive(Debug, Clone)]
pub struct SlotStore {
    state: PathBuf,
    previous: PathBuf,
    plans: PathBuf,
}

impl SlotStore {
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
    /// - [`SlotError::BadPick`] for empty names, separator carriers, and dot segments.
    fn named_slot(&self, name: &str) -> Result<PathBuf> {
        check_slot_name(name)?;
        Ok(self.plans.join(format!("{name}.json")))
    }

    /// Loads the applied manifest, treating missing slots as empty.
    ///
    /// # Errors
    ///
    /// - [`SlotError::Missing`] for present slots failing reads.
    /// - [`SlotError::Denied`] for denied slots.
    /// - [`SlotError::Unknown`] for other read failures.
    /// - [`SlotError::Version`] for unsupported versions.
    pub fn load(&self) -> Result<Bundle> {
        load_bundle(&self.state)
    }

    /// Writes the applied slot and archives history with rotation.
    ///
    /// # Errors
    ///
    /// - [`SlotError::Unknown`] for render, clock, and write failures.
    /// - [`SlotError::Missing`] for missing paths.
    /// - [`SlotError::Denied`] for denied paths.
    pub fn store(&self, bundle: &Bundle, progress: Option<&ProgressSender>) -> Result<PathBuf> {
        let _ = progress;
        let text = manifest_json(&bundle.manifest).map_err(|error| SlotError::Unknown {
            path: self.state.clone(),
            message: error.to_string(),
        })?;
        write_text(&self.state, &text)?;
        let mut stamp = system_nanos().map_err(|error| SlotError::Unknown {
            path: self.previous.clone(),
            message: error.to_string(),
        })?;
        let mut dest = self.previous.join(format!("{stamp}.json"));
        while driver::exists(&dest) {
            stamp += 1;
            dest = self.previous.join(format!("{stamp}.json"));
        }
        write_text(&dest, &text)?;
        rotate_history(&self.previous)?;
        Ok(dest)
    }

    /// Resolves one picker to its live bundle and slot kind.
    ///
    /// Absent pickers read the applied slot. `@name` reads the
    /// named slot. `%N` reads history newest-first from one.
    ///
    /// # Errors
    ///
    /// - [`SlotError::BadPick`] for absent slots, malformed and out-of-range picks.
    pub fn resolve(&self, picker: Option<&str>) -> Result<(Bundle, SlotKind)> {
        let Some(raw) = picker else {
            if !driver::exists(&self.state) {
                return Err(SlotError::BadPick {
                    input: "".to_owned(),
                });
            }
            return Ok((load_bundle(&self.state)?, SlotKind::Applied));
        };
        if let Some(name) = raw.strip_prefix('@') {
            let path = self.named_slot(name)?;
            if !driver::exists(&path) {
                return Err(SlotError::BadPick {
                    input: raw.to_owned(),
                });
            }
            return Ok((load_bundle(&path)?, SlotKind::Named(name.to_string())));
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = rest.parse().map_err(|_| SlotError::BadPick {
                input: raw.to_owned(),
            })?;
            let entries = stored_bundles(&self.previous)?;
            let total = entries.len();
            if pick < 1 || pick > total {
                return Err(SlotError::BadPick {
                    input: raw.to_owned(),
                });
            }
            let (_, bundle) =
                entries
                    .into_iter()
                    .nth(pick - 1)
                    .ok_or_else(|| SlotError::BadPick {
                        input: raw.to_owned(),
                    })?;
            return Ok((bundle, SlotKind::History(pick)));
        }
        Err(SlotError::BadPick {
            input: raw.to_owned(),
        })
    }

    /// Lists stored manifests newest first with apply picks.
    ///
    /// # Errors
    ///
    /// - [`SlotError::Missing`] for missing folders.
    /// - [`SlotError::Denied`] for denied folders.
    /// - [`SlotError::Unknown`] for other folder failures.
    pub fn list_history(&self) -> Result<Vec<HistoryEntry>> {
        Ok(stored_bundles(&self.previous)?
            .into_iter()
            .enumerate()
            .map(|(position, _)| HistoryEntry {
                index: position + 1,
            })
            .collect())
    }

    /// Writes one named slot holding its bundle manifest.
    ///
    /// # Errors
    ///
    /// - [`SlotError::BadPick`] for bad names.
    /// - [`SlotError::Unknown`] for render and write failures.
    /// - [`SlotError::Missing`] for missing paths.
    /// - [`SlotError::Denied`] for denied paths.
    pub fn store_named(&self, name: &str, bundle: &Bundle) -> Result<()> {
        let path = self.named_slot(name)?;
        let text = manifest_json(&bundle.manifest).map_err(|error| SlotError::Unknown {
            path: path.clone(),
            message: error.to_string(),
        })?;
        write_text(&path, &text)?;
        Ok(())
    }

    /// Drops one named slot.
    ///
    /// # Errors
    ///
    /// - [`SlotError::BadPick`] for absent names.
    /// - [`SlotError::Missing`] for missing paths.
    /// - [`SlotError::Denied`] for denied paths.
    /// - [`SlotError::Unknown`] for other removal failures.
    pub fn delete_named(&self, name: &str) -> Result<()> {
        let path = self.named_slot(name)?;
        if !driver::exists(&path) {
            return Err(SlotError::BadPick {
                input: ["@", name].concat(),
            });
        }
        driver::remove_file(&path).map_err(|error| SlotError::from_io(&path, error))?;
        Ok(())
    }

    /// Reports whether no applied slot reads present.
    pub fn is_first_run(&self) -> bool {
        !driver::exists(&self.state)
    }
}

/// Loads one manifest file with blob ref resolution.
///
/// Missing files read as empty. Refs carry content
/// identity, so no disk or pool check runs here.
///
/// # Errors
///
/// - [`SlotError::Missing`] for present files failing reads.
/// - [`SlotError::Denied`] for denied files.
/// - [`SlotError::Unknown`] for other read failures.
/// - [`SlotError::Corrupt`] for bad payloads.
/// - [`SlotError::Version`] for version mismatch.
fn load_bundle(path: &Path) -> Result<Bundle> {
    let bytes = match driver::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Bundle::empty());
        }
        Err(error) => {
            return Err(SlotError::from_io(path, error));
        }
    };
    let value: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| SlotError::Corrupt {
            path: path.to_path_buf(),
        })?;
    match value.get("version").and_then(serde_json::Value::as_u64) {
        Some(version) if version == u64::from(BUNDLE_VERSION) => {}
        Some(version) => {
            return Err(SlotError::Version {
                path: path.to_path_buf(),
                got: version,
            });
        }
        None => {
            return Err(SlotError::Corrupt {
                path: path.to_path_buf(),
            });
        }
    }
    let stored: Manifest = serde_json::from_value(value).map_err(|_| SlotError::Corrupt {
        path: path.to_path_buf(),
    })?;
    Ok(hydrate_bundle(&stored))
}

/// Rebuilds one bundle with ref-only blob resolution.
///
/// Blob refs carry content plus stored identity, so resolution
/// clones manifest refs without touching disk.
fn hydrate_bundle(stored: &Manifest) -> Bundle {
    let mut blobs: BTreeMap<String, BlobRef> = BTreeMap::new();
    for document in &stored.documents {
        for blob in document.data.blob_refs() {
            blobs
                .entry(blob.sha().hex())
                .or_insert_with(|| blob.clone());
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
/// - [`SlotError::Denied`] for folder listing failures beyond missing folders.
/// - [`SlotError::Unknown`] for other listing failures.
fn stored_bundles(dir: &Path) -> Result<Vec<(PathBuf, Bundle)>> {
    let mut files = match driver::read_dir(dir) {
        Ok(files) => files,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(SlotError::from_io(dir, error));
        }
    };
    files.reverse();
    let mut out = Vec::new();
    for file in files {
        let bytes = match driver::read(&file) {
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
/// - [`SlotError::Denied`] for folder listing failures beyond missing folders.
/// - [`SlotError::Unknown`] for other listing failures.
fn history_files(dir: &Path) -> Result<Vec<PathBuf>> {
    match driver::read_dir(dir) {
        Ok(files) => Ok(files),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(SlotError::from_io(dir, error)),
    }
}

/// Drops stored manifests past the kept count, oldest first.
///
/// # Errors
///
/// - [`SlotError::Missing`] for missing paths.
/// - [`SlotError::Denied`] for denied paths.
/// - [`SlotError::Unknown`] for other listing and removal failures.
fn rotate_history(dir: &Path) -> Result<()> {
    let files = history_files(dir)?;
    if files.len() > HISTORY_KEPT {
        for stale in files.iter().take(files.len() - HISTORY_KEPT) {
            driver::remove_file(stale).map_err(|error| SlotError::from_io(stale, error))?;
        }
    }
    Ok(())
}

/// Writes manifest text to one path, creating parents.
///
/// # Errors
///
/// - [`SlotError::Missing`] for missing parents and paths.
/// - [`SlotError::Denied`] for denied parents and paths.
/// - [`SlotError::Unknown`] for other write failures.
fn write_text(path: &Path, text: &str) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        driver::create_dir_all(parent).map_err(|error| SlotError::from_io(path, error))?;
    }
    driver::write(path, text.as_bytes()).map_err(|error| SlotError::from_io(path, error))
}

/// Reads wall-clock nanos for sortable archive file names.
///
/// # Errors
///
/// - system time errors for clock readings before the epoch.
fn system_nanos() -> std::result::Result<u128, std::time::SystemTimeError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_nanos())
}

/// Checks one slot name holds one file stem with no separators.
///
/// Empty names, separator carriers, and dot segments fail.
///
/// # Errors
///
/// - [`SlotError::BadPick`] for empty names, separator carriers, and dot segments.
fn check_slot_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(SlotError::BadPick {
            input: name.to_owned(),
        });
    }
    if name.contains('/') || name.contains('\\') {
        return Err(SlotError::BadPick {
            input: name.to_owned(),
        });
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(SlotError::BadPick {
            input: name.to_owned(),
        });
    }
    if Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(SlotError::BadPick {
            input: name.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::{ManifestData, ManifestDocument};
    use confit_model::routes::{Route, RouteBase};

    use confit_driver::TestGuard;

    fn test_roots(dir: &Path) -> StoreRoots {
        StoreRoots {
            config_base: dir.join("config"),
            ..Default::default()
        }
    }

    fn test_store(dir: &Path) -> SlotStore {
        SlotStore::new(&test_roots(dir))
    }

    fn text_bundle(content: &str) -> Bundle {
        match Bundle::build(
            vec![ManifestDocument::new(
                Route::new(RouteBase::Home, "note").unwrap(),
                ManifestData::Text {
                    content: content.to_string(),
                    mode: None,
                    unmanaged: false,
                },
            )],
            Vec::new(),
        ) {
            Ok(bundle) => bundle,
            Err(error) => panic!("bundle builds: {error}"),
        }
    }

    fn history_count(dir: &Path) -> usize {
        let previous = dir.join("config").join(PREVIOUS_DIR);
        match driver::read_dir(&previous) {
            Ok(files) => files.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => panic!("history lists: {error}"),
        }
    }

    #[test]
    fn first_run_holds_until_first_store() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let slots = test_store(dir.path());
        assert!(slots.is_first_run(), "absent slot reads first run");
        match slots.resolve(None) {
            Ok(_) => panic!("absent applied slot passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong absent variant: {error}"),
        }
        match slots.store(&text_bundle("v1"), None) {
            Ok(_) => assert!(!slots.is_first_run(), "stored slot ends first run"),
            Err(error) => panic!("applied slot stores: {error}"),
        }
    }

    #[test]
    fn store_rotates_history_keeping_five() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let slots = test_store(dir.path());
        for index in 1..=7 {
            match slots.store(&text_bundle(&format!("v{index}")), None) {
                Ok(_) => {}
                Err(error) => panic!("history stores v{index}: {error}"),
            }
        }
        assert_eq!(history_count(dir.path()), 5, "rotation keeps five");
        match slots.list_history() {
            Ok(entries) => {
                assert_eq!(entries.len(), 5, "listing keeps five");
                for (position, entry) in entries.iter().enumerate() {
                    assert_eq!(entry.index, position + 1, "picks count from one");
                }
            }
            Err(error) => panic!("history lists: {error}"),
        }
        match slots.resolve(Some("%1")) {
            Ok((bundle, kind)) => {
                assert_eq!(kind, SlotKind::History(1), "newest reads pick one");
                assert_eq!(bundle, text_bundle("v7"), "newest keeps last write");
            }
            Err(error) => panic!("newest resolves: {error}"),
        }
    }

    #[test]
    fn pickers_resolve_applied_named_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let slots = test_store(dir.path());
        let applied = text_bundle("applied");
        match slots.store(&applied, None) {
            Ok(_) => {}
            Err(error) => panic!("applied slot stores: {error}"),
        }
        match slots.store_named("keep", &text_bundle("named")) {
            Ok(()) => {}
            Err(error) => panic!("named slot stores: {error}"),
        }
        match slots.resolve(None) {
            Ok((bundle, kind)) => {
                assert_eq!(kind, SlotKind::Applied, "absent picker reads applied");
                assert_eq!(bundle, applied, "applied picker keeps stored bundle");
            }
            Err(error) => panic!("applied picker resolves: {error}"),
        }
        match slots.resolve(Some("@keep")) {
            Ok((bundle, kind)) => {
                assert_eq!(kind, SlotKind::Named("keep".to_string()));
                assert_eq!(bundle, text_bundle("named"));
            }
            Err(error) => panic!("named picker resolves: {error}"),
        }
        match slots.resolve(Some("%1")) {
            Ok((bundle, kind)) => {
                assert_eq!(kind, SlotKind::History(1), "history picker keeps its pick");
                assert_eq!(bundle, applied, "history picker keeps stored bundle");
            }
            Err(error) => panic!("history picker resolves: {error}"),
        }
    }

    #[test]
    fn pickers_refuse_absent_and_malformed() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let slots = test_store(dir.path());
        match slots.resolve(Some("@missing")) {
            Ok(_) => panic!("absent named slot passes"),
            Err(SlotError::BadPick { input }) => {
                assert_eq!(input, "@missing", "absent name keeps its picker")
            }
            Err(error) => panic!("wrong absent variant: {error}"),
        }
        match slots.resolve(Some("%1")) {
            Ok(_) => panic!("empty history passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong range variant: {error}"),
        }
        match slots.resolve(Some("%0")) {
            Ok(_) => panic!("zero pick passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong zero variant: {error}"),
        }
        match slots.resolve(Some("%many")) {
            Ok(_) => panic!("word pick passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong word variant: {error}"),
        }
        match slots.resolve(Some("bogus")) {
            Ok(_) => panic!("bare picker passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong bare variant: {error}"),
        }
        match slots.store_named("a/b", &text_bundle("v1")) {
            Ok(()) => panic!("separator name passes"),
            Err(SlotError::BadPick { input }) => {
                assert_eq!(input, "a/b", "separator name keeps its input")
            }
            Err(error) => panic!("wrong separator variant: {error}"),
        }
    }

    #[test]
    fn named_slots_round_trip_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let slots = test_store(dir.path());
        match slots.store_named("keep", &text_bundle("named")) {
            Ok(()) => {}
            Err(error) => panic!("named slot stores: {error}"),
        }
        match slots.resolve(Some("@keep")) {
            Ok((bundle, _)) => assert_eq!(bundle, text_bundle("named")),
            Err(error) => panic!("named slot resolves: {error}"),
        }
        match slots.delete_named("keep") {
            Ok(()) => {}
            Err(error) => panic!("named slot deletes: {error}"),
        }
        match slots.resolve(Some("@keep")) {
            Ok(_) => panic!("deleted named slot passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong deleted variant: {error}"),
        }
        match slots.delete_named("keep") {
            Ok(()) => panic!("repeat delete passes"),
            Err(SlotError::BadPick { .. }) => {}
            Err(error) => panic!("wrong repeat variant: {error}"),
        }
    }

    #[test]
    fn history_lists_newest_first_with_picks() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let slots = test_store(dir.path());
        for content in ["v1", "v2", "v3"] {
            match slots.store(&text_bundle(content), None) {
                Ok(_) => {}
                Err(error) => panic!("history stores {content}: {error}"),
            }
        }
        match slots.list_history() {
            Ok(entries) => assert_eq!(entries.len(), 3, "listing keeps every entry"),
            Err(error) => panic!("history lists: {error}"),
        }
        for (pick, want) in [(1, "v3"), (2, "v2"), (3, "v1")] {
            let picker = format!("%{pick}");
            match slots.resolve(Some(picker.as_str())) {
                Ok((bundle, kind)) => {
                    assert_eq!(kind, SlotKind::History(pick));
                    assert_eq!(bundle, text_bundle(want), "pick {pick} keeps order");
                }
                Err(error) => panic!("pick {pick} resolves: {error}"),
            }
        }
    }
}
