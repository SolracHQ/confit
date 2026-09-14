//! Error
//!
//! Failure vocabulary for core operations.

use thiserror::Error;

/// Core failure shapes.
///
/// Plan errors cover bad input plus conflicts plus render failures.
/// Io errors cover filesystem failures from the caller seam.
///
/// # Examples
///
/// ```rust
/// use confit_core::error::Error;
///
/// let error = Error::Plan("bad section".to_string());
/// assert!(matches!(error, Error::Plan(_)));
/// ```
#[derive(Debug, Error)]
pub enum Error {
    /// Plan failure with a human readable reason.
    #[error("plan error: {0}")]
    Plan(String),
    /// Filesystem failure from the caller seam.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Core result alias.
///
/// # Examples
///
/// ```rust
/// use confit_core::error::Result;
///
/// let value: Result<u32> = Ok(1);
/// assert!(matches!(value, Ok(1)));
/// ```
pub type Result<T> = std::result::Result<T, Error>;
