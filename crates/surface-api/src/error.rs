//! Error
//! Failure vocabulary for the surface.

use thiserror::Error;

/// Surface failure shapes.
#[derive(Debug, Clone, Error)]
pub enum ApiError {
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
    /// Model failure.
    #[error(transparent)]
    Model(#[from] confit_model::error::Error),
    /// Lua failure with the stack trace.
    #[error(transparent)]
    Lua(#[from] mlua::Error),
}

impl ApiError {
    /// Builds one mismatch error naming its field.
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

impl From<ApiError> for mlua::Error {
    fn from(error: ApiError) -> Self {
        mlua::Error::external(error)
    }
}

/// Surface result alias.
pub type Result<T> = std::result::Result<T, ApiError>;

/// Unwraps one Lua failure back into its surface shape.
///
/// - Foreign Lua failures ride the Lua variant.
pub(crate) fn api_of(error: mlua::Error) -> ApiError {
    match error.downcast_ref::<ApiError>().cloned() {
        Some(known) => known,
        None => ApiError::Lua(error),
    }
}
