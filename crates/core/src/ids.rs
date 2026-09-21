//! Ids
//!
//! Newtypes guarding core boundaries.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// Document path with home expansion.
///
/// Paths carry a leading tilde for home relative targets.
/// Expansion resolves the tilde against the home folder.
///
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocPath(String);

impl DocPath {
    /// Builds a document path from text.
    ///
    /// # Arguments
    ///
    /// * `path` - the path text, holding a leading tilde for home targets.
    ///
    /// # Returns
    ///
    /// The path for plan keys and warnings.
    ///
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// Reads the path as a string slice.
    ///
    /// # Returns
    ///
    /// The raw path text with any tilde intact.
    ///
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Expands a leading tilde against the home folder.
    ///
    /// # Returns
    ///
    /// The expanded path. Plain paths pass through intact.
    /// Tilde paths without a home folder pass through intact.
    ///
    pub fn expand(&self) -> PathBuf {
        let raw = self.as_str();
        let Some(rest) = raw.strip_prefix('~') else {
            return PathBuf::from(raw);
        };
        let rest = match rest.strip_prefix('/') {
            Some(stripped) => stripped,
            None => rest,
        };
        match dirs::home_dir() {
            Some(home) => home.join(rest),
            None => PathBuf::from(raw),
        }
    }
}

impl std::fmt::Display for DocPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for DocPath {
    fn from(path: &str) -> Self {
        Self(path.to_string())
    }
}

/// Disk state behind one document path.
///
/// The caller reads the path and reports the outcome.
/// Core stays free of filesystem access.
///
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Empty path. The document awaits creation.
    Absent,
    /// Disk bytes plus permission bits for the path.
    Present {
        /// Holds raw disk bytes for the path.
        bytes: Vec<u8>,
        /// Holds unix permission bits. None while the backend
        /// holds no mode, like symlinks or umask default files.
        mode: Option<u32>,
    },
    /// Failing read. Carries the raw failure detail.
    ///
    /// # Arguments
    ///
    /// * `reason` - the failure detail from the read.
    Unreadable {
        /// Holds the raw failure detail from the read.
        reason: String,
    },
}

/// Computes lowercase hex SHA-256 over bytes.
///
/// Content hashes identify blobs the way paths identify
/// destinations.
///
/// # Arguments
///
/// * `bytes` - the input bytes.
///
/// # Returns
///
/// Lowercase hex digest.
///
/// # Examples
///
/// ```rust
/// use confit_core::ids::sha256_hex;
///
/// let digest = sha256_hex(b"abc");
/// assert!(matches!(digest.starts_with("ba7816"), true));
/// ```
pub fn sha256_hex(bytes: &[u8]) -> String {
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_expands_against_home() {
        let previous = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", "/tmp/confit-home-fixture");
        }
        let expanded = DocPath::new("~/.bashrc").expand();
        let bare = DocPath::new("~").expand();
        let plain = DocPath::new("/etc/hosts").expand();
        match previous {
            Some(value) => unsafe {
                std::env::set_var("HOME", value);
            },
            None => unsafe {
                std::env::remove_var("HOME");
            },
        }
        assert_eq!(expanded, PathBuf::from("/tmp/confit-home-fixture/.bashrc"));
        assert_eq!(bare, PathBuf::from("/tmp/confit-home-fixture"));
        assert_eq!(plain, PathBuf::from("/etc/hosts"));
    }
}
