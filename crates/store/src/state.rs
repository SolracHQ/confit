//! Stored state files with picker resolution.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use confit_core::error::{
    ContextualCause, ContextualResult, ErrorCause, FilesystemError, SlotReason, WithContext,
    contextual,
};
use confit_driver as driver;

use crate::StoreRoots;

/// The number of history files kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 6;

/// The folder name for history entries.
const HISTORY_DIR: &str = "history";

/// The folder name for named states.
const STATES_DIR: &str = "states";

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
    /// The applied state.
    Applied,
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
                    return Err(contextual(SlotReason::NotNumber));
                }
            };
            if pick == 0 {
                return Err(contextual(SlotReason::OutOfRange));
            }
            return Ok(Self::History(pick));
        }
        Err(contextual(SlotReason::BareSlot))
    }
}

/// A store of history entries and named state files with picker choices.
#[derive(Debug, Clone)]
pub struct StateStore {
    history: PathBuf,
    states: PathBuf,
}

impl StateStore {
    /// Builds a state store under the config base.
    pub fn new(roots: &StoreRoots) -> Self {
        Self {
            history: roots.config_base.join(HISTORY_DIR),
            states: roots.config_base.join(STATES_DIR),
        }
    }

    /// Returns the handle for the applied state.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::MissingEntry`] when the history is empty.
    pub fn applied(&self) -> ContextualResult<StateHandle> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        files
            .into_iter()
            .next()
            .map(StateHandle::new)
            .ok_or_else(|| contextual(SlotReason::MissingEntry).with_fs_path(&self.history))
    }

    /// Returns the handle for one stored name.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::EmptyName`] when the name is empty.
    /// - [`SlotReason::NameChars`] when the name holds a forbidden character.
    /// - [`SlotReason::MissingEntry`] when the state file is absent.
    pub fn named(&self, name: &str) -> ContextualResult<StateHandle> {
        let path = self.named_path(name)?;
        checked(path)
    }

    /// Resolves one picker into its state handle.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::MissingEntry`] when the state file is absent.
    /// - [`SlotReason::OutOfRange`] when the pick is out of range.
    /// - [`FilesystemError`] when the history folder is unreadable.
    pub fn resolve(&self, picker: &Picker) -> ContextualResult<StateHandle> {
        match picker {
            Picker::Applied => self.applied(),
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                checked(path)
            }
            Picker::History(pick) => {
                let files = history_files(&self.history)?;
                if *pick >= files.len() {
                    return Err(contextual(SlotReason::OutOfRange).with_fs_path(&self.history));
                }
                let Some(path) = files.into_iter().rev().nth(*pick) else {
                    return Err(contextual(SlotReason::OutOfRange).with_fs_path(&self.history));
                };
                Ok(StateHandle::new(path))
            }
        }
    }

    /// Reads one state file.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state file is unreadable.
    pub fn read(&self, handle: &StateHandle) -> ContextualResult<Vec<u8>> {
        let path = handle.canonical();
        std::fs::read(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)
    }

    /// Creates one empty state file and returns its handle.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::BareSlot`] when the picker picks the applied slot or a
    ///   history entry.
    /// - [`SlotReason::EmptyName`] when the name is empty.
    /// - [`SlotReason::NameChars`] when the name holds a forbidden character.
    /// - [`FilesystemError`] when the state file fails to write.
    pub fn create(&self, picker: &Picker) -> ContextualResult<StateHandle> {
        match picker {
            Picker::Applied | Picker::History(_) => Err(contextual(SlotReason::BareSlot)),
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                if !path.exists() {
                    store_bytes(&path, &mut std::io::empty())?;
                }
                checked(path)
            }
        }
    }

    /// Writes one source into a state file.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state file fails to write.
    pub fn write(&self, handle: &StateHandle, mut source: impl Read) -> ContextualResult<()> {
        store_bytes(handle.canonical(), &mut source)
    }

    /// Removes one state file.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state file fails to remove.
    pub fn remove(&self, handle: &StateHandle) -> ContextualResult<()> {
        let path = handle.canonical();
        std::fs::remove_file(path)
            .map_err(ContextualCause::from)
            .with_fs_path(path)
    }

    /// Returns the history handles.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the history folder is unreadable.
    pub fn list(&self) -> ContextualResult<Vec<StateHandle>> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        Ok(files.into_iter().map(StateHandle::new).collect())
    }

    /// Lists named state handles.
    ///
    /// # Errors
    ///
    /// - [`FilesystemError`] when the state folder is unreadable.
    pub fn list_named(&self) -> ContextualResult<Vec<StateHandle>> {
        Ok(history_files(&self.states)?
            .into_iter()
            .map(StateHandle::new)
            .collect())
    }

    /// Writes one streamed source into a new history file.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::MissingEntry`] when the state file vanishes.
    /// - [`FilesystemError`] when a clock reading or a state write fails.
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

    /// Returns whether the history is empty.
    pub fn is_first_run(&self) -> bool {
        history_files(&self.history)
            .map(|files| files.is_empty())
            .unwrap_or(true)
    }

    /// Returns the file path for one validated state name.
    ///
    /// # Errors
    ///
    /// - [`SlotReason::EmptyName`] when the name is empty.
    /// - [`SlotReason::NameChars`] when the name holds a forbidden character.
    fn named_path(&self, name: &str) -> ContextualResult<PathBuf> {
        validate_state_name(name)?;
        Ok(self.states.join(format!("{name}.json")))
    }
}

/// Returns the handle for one state file that exists.
///
/// # Errors
///
/// - [`SlotReason::MissingEntry`] when the state file is absent.
fn checked(path: PathBuf) -> ContextualResult<StateHandle> {
    if !path.exists() {
        return Err(contextual(SlotReason::MissingEntry).with_fs_path(path));
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
/// - [`SlotReason::EmptyName`] when the name is empty.
/// - [`SlotReason::NameChars`] when the name holds a forbidden character.
fn validate_state_name(name: &str) -> ContextualResult<()> {
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
