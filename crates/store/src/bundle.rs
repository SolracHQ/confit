//! Bundle
//!
//! Portable bundle archives holding manifests and blobs.

use std::path::{Path, PathBuf};

use confit_core::error::Result;
use confit_core::plan::Bundle;
use confit_core::progress::ProgressSender;

pub mod file;
pub mod memory;

/// Bundle file extension imposed on explicit outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Ensures one bundle destination carries the bundle extension.
///
/// Bare paths gain the suffix, so creators always emit
/// bundles. Slot outputs never pass here and keep their
/// own names.
pub fn ensure_bundle_extension(dest: &Path) -> PathBuf {
    if dest
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(BUNDLE_EXTENSION))
    {
        dest.to_path_buf()
    } else {
        let mut name = dest.as_os_str().to_owned();
        name.push(".cb");
        PathBuf::from(name)
    }
}

/// Portable bundle archive reads and writes.
///
/// Archives hold the manifest first with blob entries after.
pub trait BundleStore {
    /// Writes one portable bundle holding manifest and blobs.
    ///
    /// Bare destinations gain the bundle extension; the
    /// returned path names the written file.
    ///
    /// # Errors
    ///
    /// Compression and write failures surface as plan errors.
    fn write(
        &self,
        bundle: &Bundle,
        dest: &Path,
        progress: Option<&ProgressSender>,
    ) -> Result<PathBuf>;

    /// Reads one portable bundle into a live bundle.
    ///
    /// # Errors
    ///
    /// Unreadable files and bad payloads fail as plan errors.
    fn read(&self, path: &Path) -> Result<Bundle>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_output_gains_cb_suffix() {
        let out = ensure_bundle_extension(Path::new("plan"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("bare output gains suffix"),
        }
    }

    #[test]
    fn cb_suffix_stays_unchanged() {
        let out = ensure_bundle_extension(Path::new("plan.cb"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.cb"),
            None => panic!("cb output stays unchanged"),
        }
    }

    #[test]
    fn cb_suffix_match_reads_case_insensitive() {
        let out = ensure_bundle_extension(Path::new("plan.CB"));
        match out.to_str() {
            Some(text) => assert_eq!(text, "plan.CB"),
            None => panic!("uppercase cb stays untouched"),
        }
    }
}
