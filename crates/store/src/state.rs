//! State files with picker resolution behind handles.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use confit_core::error::{
    ContextualCause, ContextualResult, ErrorCause, FilesystemError, SlotReason, WithContext,
    contextual,
};
use confit_driver as driver;

use crate::StoreRoots;
use crate::handles::StateHandle;

/// Stored states kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 5;

/// State file name under the config base.
const STATE_FILE: &str = "state.json";

/// History folder name under the config base.
const HISTORY_DIR: &str = "history";

/// Named state folder name under the config base.
const STATES_DIR: &str = "states";

/// State picker for stored states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Picker {
    /// The applied state.
    Applied,
    /// One named state.
    Named(String),
    /// One history pick newest-first from one.
    History(usize),
}

impl FromStr for Picker {
    type Err = ContextualCause;

    fn from_str(raw: &str) -> ContextualResult<Self> {
        if let Some(name) = raw.strip_prefix('@') {
            check_state_name(name)?;
            return Ok(Self::Named(name.to_owned()));
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = match rest.parse() {
                Ok(pick) => pick,
                Err(_) => {
                    return Err(contextual(SlotReason::NotNumber));
                }
            };
            return Ok(Self::History(pick));
        }
        Err(contextual(SlotReason::BareSlot))
    }
}

/// State files with picker resolution.
#[derive(Debug, Clone)]
pub struct StateStore {
    state: PathBuf,
    history: PathBuf,
    states: PathBuf,
}

impl StateStore {
    /// Build a file-backed state store under the config base.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            state: roots.config_base.join(STATE_FILE),
            history: roots.config_base.join(HISTORY_DIR),
            states: roots.config_base.join(STATES_DIR),
        }
    }

    /// Return the applied handle for the stored state.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::MissingEntry`] for absent states.
    pub fn applied(&self) -> ContextualResult<StateHandle> {
        checked(self.state.clone())
    }

    /// Return one named handle for one stored name.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::EmptyName`] for empty names.
    /// - [`SlotReason::NameChars`] for forbidden characters.
    /// - [`SlotReason::MissingEntry`] for absent states.
    pub fn named(&self, name: &str) -> ContextualResult<StateHandle> {
        let path = self.named_path(name)?;
        checked(path)
    }

    /// Resolve one picker to its checked handle.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::MissingEntry`] for absent states.
    /// - [`SlotReason::OutOfRange`] for out-of-range picks.
    /// - [`FilesystemError::FileMissing`] for missing folders treated as empty.
    /// - [`FilesystemError`] for unreadable folders.
    pub fn resolve(&self, picker: &Picker) -> ContextualResult<StateHandle> {
        match picker {
            Picker::Applied => self.applied(),
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                checked(path)
            }
            Picker::History(pick) => {
                let files = history_files(&self.history)?;
                if *pick < 1 || *pick > files.len() {
                    return Err(contextual(SlotReason::OutOfRange).with_fs_path(&self.history));
                }
                let Some(path) = files.into_iter().rev().nth(pick - 1) else {
                    return Err(contextual(SlotReason::OutOfRange).with_fs_path(&self.history));
                };
                Ok(StateHandle::new(path))
            }
        }
    }

    /// Read one handle into bytes.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for unreadable states.
    pub fn read(&self, handle: &StateHandle) -> ContextualResult<Vec<u8>> {
        let path = handle.canonical();
        std::fs::read(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)
    }

    /// Create one state handle with an empty file.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::BareSlot`] for history variants.
    /// - [`SlotReason::EmptyName`] for empty names.
    /// - [`SlotReason::NameChars`] for forbidden characters.
    /// - [`FilesystemError`] for state write faults.
    pub fn create(&self, picker: &Picker) -> ContextualResult<StateHandle> {
        match picker {
            Picker::Applied => {
                if !self.state.exists() {
                    store_bytes(&self.state, &mut std::io::empty())?;
                }
                checked(self.state.clone())
            }
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                if !path.exists() {
                    store_bytes(&path, &mut std::io::empty())?;
                }
                checked(path)
            }
            Picker::History(_) => Err(contextual(SlotReason::BareSlot)),
        }
    }

    /// Write one source onto one state handle.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for state write faults.
    pub fn write(&self, handle: &StateHandle, mut source: impl Read) -> ContextualResult<()> {
        store_bytes(handle.canonical(), &mut source)
    }

    /// Remove one handle file.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for removal failures.
    pub fn remove(&self, handle: &StateHandle) -> ContextualResult<()> {
        let path = handle.canonical();
        std::fs::remove_file(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)
    }

    /// List history handles newest first.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for unreadable folders beyond missing folders.
    pub fn list(&self) -> ContextualResult<Vec<StateHandle>> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        Ok(files.into_iter().map(StateHandle::new).collect())
    }

    /// List state handles.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] for unreadable folders beyond missing folders.
    pub fn list_named(&self) -> ContextualResult<Vec<StateHandle>> {
        Ok(history_files(&self.states)?
            .into_iter()
            .map(StateHandle::new)
            .collect())
    }

    /// Rotate one streamed source into a fresh history file.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::MissingEntry`] for vanishing states.
    /// - [`FilesystemError`] for clock and other write failures.
    pub fn rotate(&self, mut source: impl Read) -> ContextualResult<()> {
        let mut stamp = system_nanos()?;
        let mut destination = self.history.join(format!("{stamp}.json"));
        while destination.exists() {
            stamp += 1;
            destination = self.history.join(format!("{stamp}.json"));
        }
        store_bytes(&destination, &mut source)?;
        checked(destination)?;
        let files = history_files(&self.history)?;
        if files.len() > HISTORY_KEPT {
            for stale in files.iter().take(files.len() - HISTORY_KEPT) {
                std::fs::remove_file(stale)
                    .map_err(ContextualCause::from)
                    .with_fs_path(stale)?;
            }
        }
        Ok(())
    }

    /// Report whether this is the first run.
    pub fn is_first_run(&self) -> bool {
        !self.state.exists()
    }

    /// Resolve the state path for one validated name.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::EmptyName`] for empty names.
    /// - [`SlotReason::NameChars`] for forbidden characters.
    fn named_path(&self, name: &str) -> ContextualResult<PathBuf> {
        check_state_name(name)?;
        Ok(self.states.join(format!("{name}.json")))
    }
}

/// Return one state handle for one present file.
///
/// # Errors
///
/// - [`SlotReason::MissingEntry`] for absent states.
fn checked(path: PathBuf) -> ContextualResult<StateHandle> {
    if !path.exists() {
        return Err(contextual(SlotReason::MissingEntry).with_fs_path(path));
    }
    Ok(StateHandle::new(path))
}

/// List stored files oldest first.
///
/// # Errors
///
/// - [`FilesystemError`] for unreadable folders beyond missing folders.
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

/// Read wall-clock nanos for sortable history file names.
///
/// # Errors
///
/// - [`FilesystemError::FileUnknown`] for clock readings before the epoch.
fn system_nanos() -> ContextualResult<u128> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_nanos())
        .map_err(ContextualCause::from)
}

/// Check one state name for a file stem.
///
/// # Errors
///
/// - [`SlotReason::EmptyName`] for empty names.
/// - [`SlotReason::NameChars`] for forbidden characters.
fn check_state_name(name: &str) -> ContextualResult<()> {
    if name.is_empty() {
        return Err(contextual(SlotReason::EmptyName));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(contextual(SlotReason::NameChars));
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(contextual(SlotReason::NameChars));
    }
    if Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(contextual(SlotReason::NameChars));
    }
    Ok(())
}

/// Store one streamed source onto one state path.
///
/// # Errors
///
/// - [`FilesystemError`] for state write faults.
fn store_bytes(path: &Path, source: &mut impl Read) -> ContextualResult<()> {
    driver::StagedFile::write(path, |file| {
        std::io::copy(&mut *source, file)?;
        file.sync_all()
    })
}
