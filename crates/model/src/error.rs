//! Error
//!
//! Failure vocabulary for core operations.

use thiserror::Error;

use crate::document::StructuredFormat;

/// Core failure shapes.
#[derive(Debug, Clone, Error)]
pub enum Error {
    /// Parse failure holding the input and the wanted shape.
    #[error("invalid '{input}': want {want}")]
    Parse {
        /// The offending input value.
        input: String,
        /// The wanted shape.
        want: String,
    },
    /// Structured render failure holding the format with its reason.
    #[error("cannot render {format}: {reason}")]
    Render {
        /// The format under rendering.
        format: StructuredFormat,
        /// The serializer reason.
        reason: String,
    },
    /// Unhashable document holding the destination with the render reason.
    #[error("cannot hash '{document}': {reason}")]
    Unhashable {
        /// The destination display under hashing.
        document: String,
        /// The render reason.
        reason: String,
    },
}

/// Core result alias.
///
pub type Result<T> = std::result::Result<T, Error>;
