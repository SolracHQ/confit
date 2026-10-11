//! State files with picker resolution behind handles.

pub mod error;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use confit_driver as driver;

use crate::StoreRoots;
use crate::handles::StateHandle;
use error::{Result, StateError};

/// Stored states kept before rotation drops the oldest.
const HISTORY_KEPT: usize = 5;

/// State file name under the config base.
const STATE_FILE: &str = "state.json";

/// History folder name under the config base.
const HISTORY_DIR: &str = "history";

/// Named state folder name under the config base.
const STATES_DIR: &str = "states";

/// State picker for applied, named, and history states.
///
/// Applied is the fixed state file. Named is one `@name` file. History
/// is one `%N` pick newest-first from one.
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
    type Err = StateError;

    fn from_str(raw: &str) -> Result<Self> {
        if let Some(name) = raw.strip_prefix('@') {
            check_state_name(name).map_err(|_| StateError::BadPick {
                input: raw.to_owned(),
            })?;
            return Ok(Self::Named(name.to_owned()));
        }
        if let Some(rest) = raw.strip_prefix('%') {
            let pick: usize = rest.parse().map_err(|_| StateError::BadPick {
                input: raw.to_owned(),
            })?;
            return Ok(Self::History(pick));
        }
        Err(StateError::BadPick {
            input: raw.to_owned(),
        })
    }
}

/// State files with picker resolution.
///
/// Newest-first history with `%N` picks from one. Config base files for
/// applied, named, and history states.
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
    /// - [`StateError::BadPick`] for absent states.
    pub fn applied(&self) -> Result<StateHandle> {
        checked(self.state.clone(), String::new())
    }

    /// Return one named handle for one stored name.
    ///
    /// # Errors
    ///
    /// - [`StateError::BadPick`] for empty names, separator carriers,
    ///   dot segments, and absent states.
    pub fn named(&self, name: &str) -> Result<StateHandle> {
        let path = self.named_path(name)?;
        checked(path, name.to_owned())
    }

    /// Resolve one picker to its checked handle.
    ///
    /// # Errors
    ///
    /// - [`StateError::BadPick`] for absent states and out-of-range
    ///   picks.
    /// - [`StateError::Read`] for failed folder reads.
    pub fn resolve(&self, picker: &Picker) -> Result<StateHandle> {
        match picker {
            Picker::Applied => self.applied(),
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                checked(path, format!("@{name}"))
            }
            Picker::History(pick) => {
                let input = format!("%{pick}");
                let files = history_files(&self.history)?;
                if *pick < 1 || *pick > files.len() {
                    return Err(StateError::BadPick { input });
                }
                let path =
                    files
                        .into_iter()
                        .rev()
                        .nth(pick - 1)
                        .ok_or_else(|| StateError::BadPick {
                            input: input.clone(),
                        })?;
                Ok(StateHandle::new(path))
            }
        }
    }

    /// Stream one handle for reading.
    ///
    /// # Errors
    ///
    /// - [`StateError::Read`] for failed state reads.
    pub fn open(&self, handle: &StateHandle) -> Result<Box<dyn Read>> {
        let path = handle.canonical();
        driver::fs::open(path)
            .map(|file| file as Box<dyn Read>)
            .map_err(|error| StateError::from_io(path, error))
    }

    /// Read one handle into bytes.
    ///
    /// # Errors
    ///
    /// - [`StateError::Read`] for failed state reads.
    pub fn read(&self, handle: &StateHandle) -> Result<Vec<u8>> {
        let path = handle.canonical();
        driver::fs::read(path).map_err(|error| StateError::from_io(path, error))
    }

    /// Create one state handle with an empty file.
    ///
    /// - Applied creates the fixed state file.
    /// - Named creates one `@name` file.
    /// - History variants refuse with [`StateError::BadPick`].
    ///
    /// # Errors
    ///
    /// - [`StateError::BadPick`] for empty names, separator carriers,
    ///   dot segments, history variants, and states vanishing after
    ///   the write.
    /// - [`StateError::Write`] for state write faults.
    /// - [`StateError::WriteUnknown`] for other write failures.
    pub fn create(&self, picker: &Picker) -> Result<StateHandle> {
        match picker {
            Picker::Applied => {
                if driver::fs::is_missing(&self.state) {
                    store_bytes(&self.state, &mut std::io::empty())?;
                }
                checked(self.state.clone(), String::new())
            }
            Picker::Named(name) => {
                let path = self.named_path(name)?;
                if driver::fs::is_missing(&path) {
                    store_bytes(&path, &mut std::io::empty())?;
                }
                checked(path, format!("@{name}"))
            }
            Picker::History(pick) => Err(StateError::BadPick {
                input: format!("%{pick}"),
            }),
        }
    }

    /// Write one source onto one state handle.
    ///
    /// # Errors
    ///
    /// - [`StateError::Write`] for state write faults.
    /// - [`StateError::WriteUnknown`] for other write failures.
    pub fn write(&self, handle: &StateHandle, mut source: impl Read) -> Result<()> {
        store_bytes(handle.canonical(), &mut source)
    }

    /// Remove one handle file.
    ///
    /// # Errors
    ///
    /// - [`StateError::Write`] for state write faults.
    /// - [`StateError::WriteUnknown`] for other removal failures.
    pub fn remove(&self, handle: &StateHandle) -> Result<()> {
        let path = handle.canonical();
        driver::fs::remove_file(path).map_err(|error| StateError::from_write_io(path, error))
    }

    /// List history handles newest first.
    ///
    /// - Missing folders are empty.
    ///
    /// # Errors
    ///
    /// - [`StateError::Read`] for failed folder reads beyond missing
    ///   folders.
    pub fn list(&self) -> Result<Vec<StateHandle>> {
        let mut files = history_files(&self.history)?;
        files.reverse();
        Ok(files.into_iter().map(StateHandle::new).collect())
    }

    /// List named handles in name order.
    ///
    /// - Missing folders are empty.
    ///
    /// # Errors
    ///
    /// - [`StateError::Read`] for failed folder reads beyond missing
    ///   folders.
    pub fn list_named(&self) -> Result<Vec<StateHandle>> {
        Ok(history_files(&self.states)?
            .into_iter()
            .map(StateHandle::new)
            .collect())
    }

    /// Rotate one streamed source into a fresh history file.
    ///
    /// - The oldest file drops past the kept count.
    ///
    /// # Errors
    ///
    /// - [`StateError::BadPick`] for states vanishing after the write.
    /// - [`StateError::Write`] for state write faults.
    /// - [`StateError::WriteUnknown`] for clock and other write
    ///   failures.
    pub fn rotate(&self, mut source: impl Read) -> Result<()> {
        let mut stamp = system_nanos().map_err(|error| StateError::WriteUnknown {
            path: self.history.clone(),
            message: error.to_string(),
        })?;
        let mut destination = self.history.join(format!("{stamp}.json"));
        while driver::fs::exists(&destination) {
            stamp += 1;
            destination = self.history.join(format!("{stamp}.json"));
        }
        store_bytes(&destination, &mut source)?;
        checked(destination, String::new())?;
        let files = history_files(&self.history)?;
        if files.len() > HISTORY_KEPT {
            for stale in files.iter().take(files.len() - HISTORY_KEPT) {
                driver::fs::remove_file(stale)
                    .map_err(|error| StateError::from_write_io(stale, error))?;
            }
        }
        Ok(())
    }

    /// Report whether no applied state is present.
    pub fn is_first_run(&self) -> bool {
        driver::fs::is_missing(&self.state)
    }

    /// Resolve the state path for one validated name.
    ///
    /// # Errors
    ///
    /// - [`StateError::BadPick`] for empty names, separator carriers,
    ///   and dot segments.
    fn named_path(&self, name: &str) -> Result<PathBuf> {
        check_state_name(name)?;
        Ok(self.states.join(format!("{name}.json")))
    }
}

/// Return one state handle for one present file.
///
/// # Errors
///
/// - [`StateError::BadPick`] for absent states.
fn checked(path: PathBuf, input: String) -> Result<StateHandle> {
    if driver::fs::is_missing(&path) {
        return Err(StateError::BadPick { input });
    }
    Ok(StateHandle::new(path))
}

/// List stored files oldest first.
///
/// - Missing folders are empty.
///
/// # Errors
///
/// - [`StateError::Read`] for failed folder reads beyond missing
///   folders.
fn history_files(directory: &Path) -> Result<Vec<PathBuf>> {
    match driver::fs::read_dir(directory) {
        Ok(files) => Ok(files),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(StateError::from_io(directory, error)),
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

/// Check one state name for one file stem with no separators.
///
/// # Errors
///
/// - [`StateError::BadPick`] for empty names, separator carriers, and dot
///   segments.
fn check_state_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(StateError::BadPick {
            input: name.to_owned(),
        });
    }
    if name.contains('/') || name.contains('\\') {
        return Err(StateError::BadPick {
            input: name.to_owned(),
        });
    }
    if name == "." || name == ".." || name.contains('\0') {
        return Err(StateError::BadPick {
            input: name.to_owned(),
        });
    }
    if Path::new(name)
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(StateError::BadPick {
            input: name.to_owned(),
        });
    }
    Ok(())
}

/// Store one streamed source onto one state path.
///
/// # Errors
///
/// - [`StateError::Write`] for state write faults.
/// - [`StateError::WriteUnknown`] for other write failures.
fn store_bytes(path: &Path, source: &mut impl Read) -> Result<()> {
    driver::atomic_write(path, |pending| {
        let mut pended = driver::fs::create(pending)?;
        driver::copy_stream(&mut *source, &mut pended)?;
        pended.sync()
    })
    .map_err(|error| StateError::from_write_io(path, error))
}
