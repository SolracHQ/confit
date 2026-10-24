//! Bundle
//! Portable bundle archives.

mod pack;
mod unpack;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_store::Stores;
use confit_store::resources::Resources;

/// Bundle file extension imposed on explicit outputs.
const BUNDLE_EXTENSION: &str = "cb";

/// Bundle manifest file name inside the archive.
pub(crate) const BUNDLE_MANIFEST: &str = "manifest.json";

/// Portable bundle archives.
#[derive(Debug, Clone)]
pub struct Bundler {
    pub(crate) resources: Arc<Resources>,
}

impl Bundler {
    /// Build a bundler over shared stores.
    pub fn new(stores: &Stores) -> Self {
        Self {
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
    fn bundle_extension_pins_suffix_contract() {
        for (input, want) in [
            ("plan", "plan.cb"),
            ("plan.cb", "plan.cb"),
            ("plan.CB", "plan.CB"),
        ] {
            let out = ensure_bundle_extension(Path::new(input));
            assert_eq!(out.to_str(), Some(want));
        }
    }
}
