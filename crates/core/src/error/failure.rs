//! Cross-tier failures mixing context and terminal errors.

use std::path::Path;

use super::cause::{ErrorCause, HookReason, SlotReason};
use super::context_layer::ErrorContext;
use super::terminal::{Error, Outcome};

/// Terminal mapping for results carrying context errors.
pub trait Terminal<T> {
    /// Map one failure into its bundle read terminal.
    fn read_bundle(self, path: &Path) -> Outcome<T>;
    /// Map one failure into its bundle write terminal.
    fn write_bundle(self, path: &Path) -> Outcome<T>;
    /// Map one failure into its member unpack terminal.
    fn unpack_member(self, member: &str, required_by: Option<&str>) -> Outcome<T>;
    /// Map one failure into its module open terminal.
    fn open_module(self, name: &str) -> Outcome<T>;
    /// Map one failure into its hook run terminal.
    fn hook_failed(self, argv: &str, reason: &HookReason) -> Outcome<T>;
    /// Map one failure into its hook spawn terminal.
    fn spawn_hook(self, argv: &str) -> Outcome<T>;
    /// Map one failure into its profile open terminal.
    fn open_profile(self, path: &Path) -> Outcome<T>;
    /// Map one failure into its project load terminal.
    fn load_project(self, path: &Path) -> Outcome<T>;
    /// Map one failure into its slot delete terminal.
    fn delete_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T>;
    /// Map one failure into its slot apply terminal.
    fn apply_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T>;
    /// Map one failure into its slot export terminal.
    fn export_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T>;
    /// Map one failure into its slot bundle terminal.
    fn bundle_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T>;
    /// Map one failure into its hook log write terminal.
    fn write_hook_log(self, path: &Path) -> Outcome<T>;
    /// Map one failure into its destination write terminal.
    fn write_destination(self, path: &Path) -> Outcome<T>;
    /// Map one failure into its destination removal terminal.
    fn remove_destination(self, path: &Path) -> Outcome<T>;
}

impl<T, E> Terminal<T> for std::result::Result<T, E>
where
    E: Into<ErrorContext>,
{
    /// Map one failure into its bundle read terminal.
    fn read_bundle(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::read_bundle(path.to_path_buf(), error.into()))
    }

    /// Map one failure into its bundle write terminal.
    fn write_bundle(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::write_bundle(path.to_path_buf(), error.into()))
    }

    /// Map one failure into its member unpack terminal.
    fn unpack_member(self, member: &str, required_by: Option<&str>) -> Outcome<T> {
        self.map_err(|error| {
            Error::unpack_member(
                member.to_owned(),
                required_by.map(str::to_owned),
                error.into(),
            )
        })
    }

    /// Map one failure into its module open terminal.
    fn open_module(self, name: &str) -> Outcome<T> {
        self.map_err(|error| Error::open_module(name.to_owned(), error.into()))
    }

    /// Map one failure into its hook run terminal.
    fn hook_failed(self, argv: &str, reason: &HookReason) -> Outcome<T> {
        self.map_err(|error| Error::hook_failed(argv.to_owned(), reason.clone(), error.into()))
    }

    /// Map one failure into its hook spawn terminal.
    fn spawn_hook(self, argv: &str) -> Outcome<T> {
        self.map_err(|error| Error::spawn_hook(argv.to_owned(), error.into()))
    }

    /// Map one failure into its profile open terminal.
    fn open_profile(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::open_profile(path.to_path_buf(), error.into()))
    }

    /// Map one failure into its project load terminal.
    fn load_project(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::load_project(path.to_path_buf(), error.into()))
    }

    /// Map one failure into its slot delete terminal.
    fn delete_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T> {
        self.map_err(|_| Error::delete_slot(slot.to_owned(), reason.clone()))
    }

    /// Map one failure into its slot apply terminal.
    fn apply_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T> {
        self.map_err(|_| Error::apply_slot(slot.to_owned(), reason.clone()))
    }

    /// Map one failure into its slot export terminal.
    fn export_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T> {
        self.map_err(|_| Error::export_slot(slot.to_owned(), reason.clone()))
    }

    /// Map one failure into its slot bundle terminal.
    fn bundle_slot(self, slot: &str, reason: &SlotReason) -> Outcome<T> {
        self.map_err(|_| Error::bundle_slot(slot.to_owned(), reason.clone()))
    }

    /// Map one failure into its hook log write terminal.
    fn write_hook_log(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::write_hook_log(path.to_path_buf(), error.into()))
    }

    /// Map one failure into its destination write terminal.
    fn write_destination(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::write_destination(path.to_path_buf(), error.into()))
    }

    /// Map one failure into its destination removal terminal.
    fn remove_destination(self, path: &Path) -> Outcome<T> {
        self.map_err(|error| Error::remove_destination(path.to_path_buf(), error.into()))
    }
}

/// Non-blocking diff warning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffWarning {
    /// The route under stating.
    route: String,
    /// The log chain.
    chain: ErrorContext,
}

impl DiffWarning {
    /// Build one diff warning naming its route with its chain.
    pub fn warn(route: impl Into<String>, chain: ErrorContext) -> Self {
        Self {
            route: route.into(),
            chain,
        }
    }

    /// Render one honest warning line.
    pub fn summary(&self) -> String {
        format!(
            "Cannot state the difference for '{}': {}",
            self.route,
            self.chain.root_cause()
        )
    }

    /// Report one log chain for remediation.
    pub fn chain(&self) -> &ErrorContext {
        &self.chain
    }
}

/// Cross-tier failure for operations mixing context and terminal errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Mid error, context continues above.
    Mid(ErrorContext),
    /// Complete terminal, travels untouched.
    Terminal(Error),
}

/// Cross-tier failure result alias.
pub type Mixed<T> = std::result::Result<T, Failure>;

impl From<ErrorContext> for Failure {
    /// Wrap one context error.
    fn from(error: ErrorContext) -> Self {
        Self::Mid(error)
    }
}

impl From<Error> for Failure {
    /// Wrap one complete terminal untouched.
    fn from(terminal: Error) -> Self {
        Self::Terminal(terminal)
    }
}

impl From<ErrorCause> for Failure {
    /// Wrap one bare cause as a context error.
    fn from(cause: ErrorCause) -> Self {
        Self::Mid(ErrorContext::from(cause))
    }
}

impl std::fmt::Display for Failure {
    /// Render one inner failure.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mid(error) => write!(formatter, "{error}"),
            Self::Terminal(terminal) => write!(formatter, "{}", terminal.summary()),
        }
    }
}

impl std::error::Error for Failure {
    /// Report one inner failure.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Mid(error) => Some(error),
            Self::Terminal(terminal) => Some(terminal),
        }
    }
}
