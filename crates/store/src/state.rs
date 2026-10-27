//! State files with picker resolution behind handles.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use confit_core::error::locate::Locate;
use confit_core::error::{ErrorCause, ErrorContext, Result, SlotReason, fail};
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
    type Err = ErrorContext;

    fn from_str(raw: &str) -> Result<Self> {
        if let Some(name) = raw.strip_prefix('@') {
            check_state_name(name)?;
            return Ok(Self::Named(name.to_owned()));
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = match rest.parse() {
                Ok(pick) => pick,
                Err(_) => return fail(ErrorCause::SlotReason(SlotReason::NotNumber)),
            };
            return Ok(Self::History(pick));
        }
        fail(ErrorCause::SlotReason(SlotReason::BareSlot))
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
    /// - [`confit_core::error::SlotReason`] for absent states.
    pub fn applied(&self) -> Result<StateHandle> {
        checked(self.state.clone())
    }

    /// Return one named handle for one stored name.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::SlotReason`] for bad names and absent states.
    pub fn named(&self, name: &str) -> Result<StateHandle> {
        let path = self.named_path(name)?;
        checked(path)
    }

    /// Resolve one picker to its checked handle.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::SlotReason`] for absent states and out-of-range picks.
    /// - [`confit_core::error::FilesystemError`] for unreadable folders.
    pub fn resolve(&self, picker: &Picker) -> Result<StateHandle> {
        match picker {
            Picker::Applied => self.applied(),
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                checked(path)
            }
            Picker::History(pick) => {
                let files = history_files(&self.history)?;
                if *pick < 1 || *pick > files.len() {
                    return fail(ErrorCause::SlotReason(SlotReason::OutOfRange))
                        .path(self.history.clone());
                }
                let Some(path) = files.into_iter().rev().nth(pick - 1) else {
                    return fail(ErrorCause::SlotReason(SlotReason::OutOfRange))
                        .path(self.history.clone());
                };
                Ok(StateHandle::new(path))
            }
        }
    }

    /// Read one handle into bytes.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for unreadable states.
    pub fn read(&self, handle: &StateHandle) -> Result<Vec<u8>> {
        let path = handle.canonical();
        std::fs::read(path).path(path)
    }

    /// Create one state handle with an empty file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::SlotReason`] for bad names and history variants.
    /// - [`confit_core::error::FilesystemError`] for state write faults.
    pub fn create(&self, picker: &Picker) -> Result<StateHandle> {
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
            Picker::History(_) => fail(ErrorCause::SlotReason(SlotReason::BareSlot)),
        }
    }

    /// Write one source onto one state handle.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for state write faults.
    pub fn write(&self, handle: &StateHandle, mut source: impl Read) -> Result<()> {
        store_bytes(handle.canonical(), &mut source)
    }

    /// Remove one handle file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for removal failures.
    pub fn remove(&self, handle: &StateHandle) -> Result<()> {
        let path = handle.canonical();
        std::fs::remove_file(path).path(path)
    }

    /// List history handles newest first.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for unreadable folders beyond missing folders.
    pub fn list(&self) -> Result<Vec<StateHandle>> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        Ok(files.into_iter().map(StateHandle::new).collect())
    }

    /// List state handles.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::FilesystemError`] for unreadable folders beyond missing folders.
    pub fn list_named(&self) -> Result<Vec<StateHandle>> {
        Ok(history_files(&self.states)?
            .into_iter()
            .map(StateHandle::new)
            .collect())
    }

    /// Rotate one streamed source into a fresh history file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::SlotReason`] for vanishing states.
    /// - [`confit_core::error::FilesystemError`] for clock and other write failures.
    pub fn rotate(&self, mut source: impl Read) -> Result<()> {
        let mut stamp = system_nanos().path(self.history.clone())?;
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
                std::fs::remove_file(stale).path(stale)?;
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
    /// - [`confit_core::error::SlotReason`] for bad names.
    fn named_path(&self, name: &str) -> Result<PathBuf> {
        check_state_name(name)?;
        Ok(self.states.join(format!("{name}.json")))
    }
}

/// Return one state handle for one present file.
///
/// # Errors
///
/// - [`confit_core::error::SlotReason`] for absent states.
fn checked(path: PathBuf) -> Result<StateHandle> {
    if !path.exists() {
        return fail(ErrorCause::SlotReason(SlotReason::MissingEntry)).path(path);
    }
    Ok(StateHandle::new(path))
}

/// List stored files oldest first.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for unreadable folders beyond missing folders.
fn history_files(directory: &Path) -> Result<Vec<PathBuf>> {
    match driver::fs::read_dir(directory) {
        Ok(files) => Ok(files),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => fail(error).path(directory),
    }
}

/// Read wall-clock nanos for sortable history file names.
///
/// # Errors
///
/// - [`std::time::SystemTimeError`] for clock readings before the epoch.
fn system_nanos() -> std::result::Result<u128, std::time::SystemTimeError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|span| span.as_nanos())
}

/// Check one state name for a file stem.
///
/// # Errors
///
/// - [`confit_core::error::SlotReason`] for bad names.
fn check_state_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return fail(ErrorCause::SlotReason(SlotReason::EmptyName));
    }
    if name.contains('/') || name.contains('\\') {
        return fail(ErrorCause::SlotReason(SlotReason::NameChars));
    }
    if name == "." || name == ".." || name.contains('\0') {
        return fail(ErrorCause::SlotReason(SlotReason::NameChars));
    }
    if Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return fail(ErrorCause::SlotReason(SlotReason::NameChars));
    }
    Ok(())
}

/// Store one streamed source onto one state path.
///
/// # Errors
///
/// - [`confit_core::error::FilesystemError`] for state write faults.
fn store_bytes(path: &Path, source: &mut impl Read) -> Result<()> {
    driver::StagedFile::write(path, |file| {
        std::io::copy(&mut *source, file)?;
        file.sync_all()
    })
    .path(path)
}
