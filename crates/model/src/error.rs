//! Error
//! Failure vocabulary for core operations.

use thiserror::Error;

use crate::document::StructuredFormat;
use crate::routes::SegmentFault;

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
    /// Route jail failure.
    #[error("invalid '{input}': want valid relative segments ({fault})")]
    Segment {
        /// The offending relative text.
        input: String,
        /// Jail fault for the offense.
        fault: SegmentFault,
    },
}

/// Core result alias.
pub type Result<T> = std::result::Result<T, Error>;
