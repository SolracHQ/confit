//! Error
//! Failure vocabulary for Lua binding.

use std::fmt::{Display, Formatter};

/// Binding failure shapes.
///
/// The registry reports duplicates through this type
/// and Lua failures travel through it.
#[derive(Debug, Clone)]
pub enum BindingError {
    /// Repeated registration.
    Duplicate {
        /// The repeated face name.
        name: String,
    },
    /// Failed value conversion.
    Conversion {
        /// The conversion failure detail.
        detail: String,
    },
    /// Lua failure.
    Lua(mlua::Error),
}

impl Display for BindingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate { name } => {
                write!(formatter, "face '{name}' is already registered")
            }
            Self::Conversion { detail } => {
                write!(formatter, "cannot convert value: {detail}")
            }
            Self::Lua(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for BindingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Lua(error) => Some(error),
            _ => None,
        }
    }
}

impl From<mlua::Error> for BindingError {
    fn from(error: mlua::Error) -> Self {
        Self::Lua(error)
    }
}

impl From<BindingError> for mlua::Error {
    fn from(error: BindingError) -> Self {
        match error {
            BindingError::Lua(inner) => inner,
            other => Self::external(other),
        }
    }
}

/// Binding result alias.
pub type Result<T> = std::result::Result<T, BindingError>;
