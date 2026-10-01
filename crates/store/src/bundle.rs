//! Bundle
//!
//! Portable bundle archives holding manifests and blobs.

pub mod codec;
pub mod error;
mod pack;
mod unpack;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::archive::ArchiveStore;
use crate::blob::BlobStore;
use crate::resources::Resources;

/// Bundle file extension imposed on explicit outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Bundle manifest file name inside the archive.
pub(crate) const BUNDLE_MANIFEST: &str = "manifest.json";

/// Bundle blob folder prefix inside the archive.
pub(crate) const BUNDLE_BLOBS_PREFIX: &str = "blobs/";

/// Portable bundle archive reads and writes.
///
/// Archives hold the manifest first with blob entries after.
/// Writes source blob bytes from the pool with fallback
/// across homes. Reads stage entries through the archive
/// spill into the cache.
#[derive(Debug, Clone)]
pub struct BundleStore {
    pub(crate) archives: Arc<ArchiveStore>,
    pub(crate) blobs: Arc<BlobStore>,
    pub(crate) resources: Arc<Resources>,
}

impl BundleStore {
    /// Builds a bundle store over shared archive and blob stores.
    ///
    /// All three handles arrive shared from store construction.
    pub fn new(
        archives: Arc<ArchiveStore>,
        blobs: Arc<BlobStore>,
        resources: Arc<Resources>,
    ) -> Self {
        Self {
            archives,
            blobs,
            resources,
        }
    }
}

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
