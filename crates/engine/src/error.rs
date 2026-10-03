//! Error
//!
//! Evaluation failure vocabulary.

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
