//! Logging
//!
//! File logging setup for collision diagnostics. Shared path picks one
//! default log file so every run stays inspectable.

use std::path::Path;
use std::path::PathBuf;

use crate::error::Error;
use crate::error::Result;

/// Initializes file logging for collision diagnostics.
///
/// # Arguments
///
/// * `log_file` - the log file override, holding `None` for the default temp path.
///
/// # Returns
///
/// The log file path for stderr display.
///
/// # Errors
///
/// Log file creation plus logger init failures yield store errors.
pub fn init_logging(log_file: Option<&Path>) -> Result<PathBuf> {
    let path = log_file.map_or_else(
        || std::env::temp_dir().join(format!("confit-{}.log", std::process::id())),
        Path::to_path_buf,
    );
    fern::Dispatch::new()
        .level(log::LevelFilter::Debug)
        .chain(
            fern::log_file(&path)
                .map_err(|error| Error::Store(format!("log file '{}': {error}", path.display())))?,
        )
        .apply()
        .map_err(|error| Error::Store(format!("log init: {error}")))?;
    Ok(path)
}
