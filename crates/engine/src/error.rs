//! Error
//! Define evaluation failure vocabulary.

use thiserror::Error;

/// Optional hint suffix rendering on its own line.
struct Hint<'a>(Option<&'a str>);

impl std::fmt::Display for Hint<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(hint) = self.0 {
            write!(formatter, "\nhint: {hint}")?;
        }
        Ok(())
    }
}

/// Engine failure shapes.
#[derive(Debug, Clone, Error)]
pub enum EngineError {
    /// Wrong-type value.
    #[error("invalid {field} '{received}': want {want}")]
    Mismatch {
        /// The value field under reading.
        field: String,
        /// The wanted shape.
        want: String,
        /// The received type word.
        received: String,
    },
    /// Unknown module.
    #[error("unknown module '{name}'")]
    UnknownModule {
        /// The requested module name.
        name: String,
    },
    /// Require cycle.
    #[error("require cycle: {loop_}")]
    Cycle {
        /// The module loop.
        loop_: String,
    },
    /// Jail escape.
    #[error("{caller}('{input}'): {reason}")]
    Escape {
        /// The calling verb.
        caller: String,
        /// The offending input.
        input: String,
        /// The escape reason.
        reason: String,
    },
    /// Open failure.
    #[error(transparent)]
    Open(#[from] confit_store::resources::error::ResourceError),
    /// Blob failure.
    #[error(transparent)]
    Blob(#[from] confit_store::blob::error::BlobError),
    /// Fetch failure.
    #[error(transparent)]
    Fetch(#[from] confit_store::fetch::error::FetchError),
    /// Binding failure.
    #[error(transparent)]
    Binding(#[from] confit_binding::error::BindingError),
    /// Model failure.
    #[error(transparent)]
    Model(#[from] confit_model::error::Error),
    /// Repeated declaration.
    #[error("profile: document '{display}' is declared more than once")]
    Duplicate {
        /// The repeated destination display.
        display: String,
    },
    /// Defined-twice config.
    #[error("config '{name}' is already defined")]
    Defined {
        /// The repeated config name.
        name: String,
    },
    /// Defined-twice hook.
    #[error("hook '{id}' is already defined")]
    DuplicateHook {
        /// The repeated hook identifier.
        id: String,
    },
    /// Missing hook patch target.
    #[error("hook patch target '{id}' matches no hook")]
    UnknownHookTarget {
        /// The missing hook identifier.
        id: String,
    },
    /// Dangling requirement holding its hint.
    #[error("config \"{name}\" requires \"{target}\" config{hint}", hint = Hint(hint.as_deref()))]
    Require {
        /// The requiring config name.
        name: String,
        /// The missing required config name.
        target: String,
        /// The remediation hint.
        hint: Option<String>,
    },
    /// Changed condition.
    #[error("hook changed condition shows unknown document '{route}'")]
    ChangedUnknown {
        /// The unknown document display.
        route: String,
    },
    /// Rc guard.
    #[error("document '{display}' {section} holds 'changed' (hooks only)")]
    ChangedInRc {
        /// The offending document display.
        display: String,
        /// The offending rc section name.
        section: String,
    },
    /// Patched target.
    #[error("patch target '{display}' holds '{base}' under '{patch}' patches")]
    Structured {
        /// The patch target display.
        display: String,
        /// The base shape name.
        base: String,
        /// The patch format name.
        patch: String,
    },
    /// Missing patch target.
    #[error("patch target '{display}' matches no document")]
    UnknownTarget {
        /// The missing target display.
        display: String,
    },
    /// File read failure.
    #[error("cannot read '{path}': {reason}")]
    Read {
        /// The unreadable file path.
        path: String,
        /// The read failure reason.
        reason: String,
    },
    /// Non-UTF8 file.
    #[error("'{path}' holds non-UTF8 bytes")]
    Utf8 {
        /// The offending file path.
        path: String,
    },
    /// Project config failure.
    #[error("project '{path}': {reason}")]
    Project {
        /// The offending project path.
        path: String,
        /// The failure reason.
        reason: String,
    },
    /// Lua failure.
    #[error(transparent)]
    Lua(mlua::Error),
}

impl EngineError {
    /// Build one mismatch error with its field.
    pub fn mismatch(
        field: impl Into<String>,
        want: impl Into<String>,
        received: impl Into<String>,
    ) -> Self {
        Self::Mismatch {
            field: field.into(),
            want: want.into(),
            received: received.into(),
        }
    }
}

impl From<mlua::Error> for EngineError {
    fn from(error: mlua::Error) -> Self {
        if let Some(known) = error.downcast_ref::<Self>().cloned() {
            return known;
        }
        Self::Lua(error)
    }
}

impl From<confit_binding::value::Mismatch> for EngineError {
    fn from(error: confit_binding::value::Mismatch) -> Self {
        Self::mismatch(error.path, error.want, error.found)
    }
}

impl From<EngineError> for mlua::Error {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::Lua(inner) => inner,
            other => Self::external(other),
        }
    }
}

/// Engine result alias.
pub type Result<T> = std::result::Result<T, EngineError>;
