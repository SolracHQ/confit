//! Portable bundle archives.

mod pack;
mod unpack;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use confit_store::Stores;
use confit_store::resources::Resources;

/// The bundle extension that an explicit output carries.
const BUNDLE_EXTENSION: &str = "cb";

/// The archive member name of the bundle manifest.
pub(crate) const BUNDLE_MANIFEST: &str = "manifest.json";

/// A reader and writer for portable bundle archives.
#[derive(Debug, Clone)]
pub struct Bundler {
    /// The shared resource store.
    pub(crate) resources: Arc<Resources>,
}

impl Bundler {
    /// Builds one bundler that shares the resource store with the run.
    pub fn new(stores: &Stores) -> Self {
        Self {
            resources: stores.resources(),
        }
    }
}

/// Returns the destination with a `.cb` extension.
pub fn with_bundle_extension(dest: &Path) -> PathBuf {
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
    fn bundle_extension_adds_a_suffix_to_a_bare_path() {
        for (input, want) in [
            ("plan", "plan.cb"),
            ("plan.cb", "plan.cb"),
            ("plan.CB", "plan.CB"),
        ] {
            let out = with_bundle_extension(Path::new(input));
            assert_eq!(out.to_str(), Some(want));
        }
    }
}
