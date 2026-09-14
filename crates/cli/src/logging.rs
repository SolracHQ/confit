//! Logging
//!
//! File log setup for collision lines.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// File sink receiving collision lines from the engine.
struct FileSink {
    /// Guarded append handle for the log file.
    file: Mutex<File>,
}

impl log::Log for FileSink {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        if let Ok(mut file) = self.file.lock() {
            let _ = writeln!(file, "{}", record.args());
        }
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

/// Holds the process-wide file sink once installed.
static SINK: OnceLock<FileSink> = OnceLock::new();

/// Resolves the log path, defaulting to a per-process temp file.
///
/// # Arguments
///
/// * `log_file` - the override path, holding `None` for the default.
///
/// # Returns
///
/// The override, else `tempdir/confit-{pid}.log`.
///
/// # Examples
///
/// ```rust
/// use confit_cli::logging::resolve_path;
/// use std::path::Path;
///
/// let path = resolve_path(Some(Path::new("/tmp/confit-test.log")));
/// assert!(matches!(path.to_str(), Some("/tmp/confit-test.log")));
/// ```
pub fn resolve_path(log_file: Option<&Path>) -> PathBuf {
    if let Some(path) = log_file {
        return path.to_path_buf();
    }
    std::env::temp_dir().join(format!("confit-{}.log", std::process::id()))
}

/// Installs file logging and reports the log path back.
///
/// # Arguments
///
/// * `log_file` - the override path, holding `None` for the default.
///
/// # Returns
///
/// The log path for the trailing `log: <path>` line.
///
/// # Errors
///
/// Log file creation plus logger install failures surface as io errors.
///
/// # Examples
///
/// ```rust
/// use confit_cli::logging::resolve_path;
///
/// let path = resolve_path(None);
/// assert!(matches!(path.to_str(), Some(_)));
/// ```
pub fn init(log_file: Option<&Path>) -> std::io::Result<PathBuf> {
    let path = resolve_path(log_file);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    if SINK
        .set(FileSink {
            file: Mutex::new(file),
        })
        .is_ok()
    {
        let sink = SINK.get();
        if let Some(sink) = sink {
            let _ = log::set_logger(sink);
            log::set_max_level(log::LevelFilter::Debug);
        }
    }
    Ok(path)
}
