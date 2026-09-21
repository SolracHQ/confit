//! Error
//!
//! Failure vocabulary for core operations.

use thiserror::Error;

/// Core failure shapes.
///
/// Plan errors cover bad input plus conflicts plus render failures.
/// Io errors cover filesystem failures from the caller seam.
///
#[derive(Debug, Error)]
pub enum Error {
    /// Plan failure with a human readable reason.
    #[error("{0}")]
    Plan(String),
    /// Filesystem failure from the caller seam.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Core result alias.
///
pub type Result<T> = std::result::Result<T, Error>;
