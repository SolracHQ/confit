//! Error
//!
//! Failure vocabulary for the surface. Programmer errors raise
//! as structured variants, soft failures answer nil with reason
//! at the call.

use thiserror::Error;

/// Surface failure shapes.
#[derive(Debug, Clone, Error)]
pub enum ApiError {
    /// Wrong-type value.
    #[error("invalid {slot} '{received}': want {want}")]
    Mismatch {
        /// The value slot under reading.
        slot: String,
        /// The wanted shape.
        want: String,
        /// The received type word.
        received: String,
    },
    /// Repeated config name.
    #[error("config '{name}' is already defined")]
    Defined {
        /// The repeated collector name.
        name: String,
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
    /// Missing segments.
    #[error("{caller} expects one path segment at least")]
    EmptySegments {
        /// The calling verb.
        caller: String,
    },
    /// Seal failure.
    #[error(transparent)]
    Seal(#[from] confit_store::resources::error::ResourceError),
    /// Fetch failure.
    #[error(transparent)]
    Fetch(#[from] confit_store::fetch::error::FetchError),
    /// Model failure.
    #[error(transparent)]
    Model(#[from] confit_model::error::Error),
    /// Lua failure with the stack trace.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}

impl From<ApiError> for mlua::Error {
    fn from(error: ApiError) -> Self {
        mlua::Error::external(error)
    }
}

/// Surface result alias.
pub type Result<T> = std::result::Result<T, ApiError>;
