//! Path
//!
//! Project root plus document path resolution.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Resolves the effective project root for a CLI invocation.
///
/// # Arguments
///
/// * `root` - flag value holding the explicit root.
/// * `profile` - profile path holding the fallback parent.
///
/// # Returns
///
/// The explicit root when present, else the profile parent directory.
///
/// # Errors
///
/// Fails with `Error::Config` when the flag is empty and the profile path holds an empty parent.
pub fn resolve_root(root: &Option<PathBuf>, profile: &Path) -> Result<PathBuf> {
    if let Some(root) = root {
        return Ok(root.clone());
    }
    match profile.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => Ok(parent.to_path_buf()),
        _ => Err(Error::Config(format!(
            "profile '{}' has no parent directory: pass --root explicitly",
            profile.display()
        ))),
    }
}

/// Resolves a leading tilde against the home folder.
///
/// # Arguments
///
/// * `path` - the raw document path, holding a leading tilde where applicable.
///
/// # Returns
///
/// The expanded path, holding the literal path for plain inputs.
pub(crate) fn expand_tilde(path: &str) -> PathBuf {
    let rest = path.strip_prefix("~/").or_else(|| path.strip_prefix('~'));
    match rest {
        Some(rest) => match directories::BaseDirs::new() {
            Some(base) => base.home_dir().join(rest),
            None => PathBuf::from(path),
        },
        None => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn home_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn tilde_expands_via_home() {
        let _guard = home_lock()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let dir = tempfile::tempdir().expect("temp home");
        let previous = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", dir.path());
        }
        let expanded = expand_tilde("~/dotfile");
        let bare = expand_tilde("~");
        match previous {
            Some(value) => unsafe {
                std::env::set_var("HOME", value);
            },
            None => unsafe {
                std::env::remove_var("HOME");
            },
        }
        assert_eq!(expanded, dir.path().join("dotfile"));
        assert_eq!(bare, dir.path().join(""));
    }
}
