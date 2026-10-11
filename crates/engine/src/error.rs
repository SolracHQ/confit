//! Error
//! Define evaluation failure vocabulary.

use thiserror::Error;

use confit_surface_api::ApiError;

/// Engine failure shapes.
#[derive(Debug, Clone, Error)]
pub enum EngineError {
    /// Surface failure.
    #[error(transparent)]
    Surface(#[from] ApiError),
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
    /// Changed gate.
    #[error("hook changed gate shows unknown document '{route}'")]
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
    /// Lua failure.
    #[error(transparent)]
    Lua(mlua::Error),
}

impl From<mlua::Error> for EngineError {
    fn from(error: mlua::Error) -> Self {
        if let Some(known) = error.downcast_ref::<ApiError>().cloned() {
            return Self::Surface(known);
        }
        if let Some(known) = error.downcast_ref::<Self>().cloned() {
            return known;
        }
        Self::Lua(error)
    }
}

impl From<EngineError> for mlua::Error {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::Surface(inner) => inner.into(),
            EngineError::Lua(inner) => inner,
            other => Self::external(other),
        }
    }
}

/// Engine result alias.
pub type Result<T> = std::result::Result<T, EngineError>;
