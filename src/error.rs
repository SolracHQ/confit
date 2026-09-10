//! Defines the crate-wide error type and result alias.
//!
//! Provides one variant per failure domain. Layers map internal failures
//! onto these variants at the boundary through this enum; callers match
//! on cases they handle.
use thiserror::Error;

/// Defines failure domains for the whole crate.
///
/// Carries messages with coarse context; fine-grained context lives in
/// logs and plan debug sections alongside errors.
#[derive(Debug, Error)]
pub enum Error {
    /// Covers two contributions or artifacts failing to combine.
    ///
    /// Raised for structural conflicts alone (matching path with differing
    /// kind). Order-resolved shadowing merges normally.
    #[error("merge error: {0}")]
    Merge(String),
    /// Covers reading or writing plans or state failing.
    #[error("store error: {0}")]
    Store(String),
    /// Covers Lua evaluation or Lua value conversion failures.
    ///
    /// Spans function-values in artifact data plus unrecognized fields;
    /// the message names tool and field.
    #[error("lua error: {0}")]
    Lua(String),
    /// Covers orchestration failures (absent profile section, unrecognized
    /// shell).
    #[error("plan error: {0}")]
    Plan(String),
    /// Covers malformed CLI input or profile return tables.
    #[error("config error: {0}")]
    Config(String),
    /// Covers filesystem IO failures.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Covers plan/state serialization failures.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Provides the crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;
