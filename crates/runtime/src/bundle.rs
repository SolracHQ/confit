//! Bundle
//! Portable bundle archives.

pub mod codec;
mod pack;
mod unpack;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_store::Stores;
use confit_store::archive::ArchiveStore;
use confit_store::blob::BlobStore;
use confit_store::resources::Resources;

/// Bundle file extension imposed on explicit outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Bundle manifest file name inside the archive.
pub(crate) const BUNDLE_MANIFEST: &str = "manifest.json";

/// Bundle blob folder prefix inside the archive.
pub(crate) const BUNDLE_BLOBS_PREFIX: &str = "blobs/";

/// Portable bundle archives.
#[derive(Debug, Clone)]
pub struct Bundler {
    pub(crate) archives: Arc<ArchiveStore>,
    pub(crate) blobs: Arc<BlobStore>,
    pub(crate) resources: Arc<Resources>,
}

impl Bundler {
    /// Build a bundler over shared stores.
    pub fn new(stores: &Stores) -> Self {
        Self {
            archives: stores.archives(),
            blobs: stores.blobs(),
            resources: stores.resources(),
        }
    }
}

/// Ensure one bundle destination with the bundle extension.
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
