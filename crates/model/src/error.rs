//! Error
//! Failure vocabulary for core operations.

use thiserror::Error;

use crate::document::StructuredFormat;
use crate::routes::RouteBase;

/// Core failure shapes.
#[derive(Debug, Clone, Error)]
pub enum Error {
    /// Parse failure.
    #[error("invalid '{input}': want {want}")]
    Parse {
        /// The offending input value.
        input: String,
        /// The wanted shape.
        want: String,
    },
    /// Structured render failure.
    #[error("cannot render {format}: {reason}")]
    Render {
        /// The format under rendering.
        format: StructuredFormat,
        /// The serializer reason.
        reason: String,
    },
    /// Unhashable document.
    #[error("cannot hash '{document}': {reason}")]
    Unhashable {
        /// The destination display under hashing.
        document: String,
        /// The render reason.
        reason: String,
    },
    /// Blob seal failure.
    #[error("cannot seal '{document}': {reason}")]
    Seal {
        /// The destination display under sealing.
        document: String,
        /// The store reason.
        reason: String,
    },
    /// Unresolvable base folder.
    #[error("cannot resolve '{base}' folder", base = base.name())]
    Base {
        /// The base under resolving.
        base: RouteBase,
    },
}

/// Core result alias.
pub type Result<T> = std::result::Result<T, Error>;
