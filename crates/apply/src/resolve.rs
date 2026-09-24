//! Route expansion against host folders and memory fakes.

use std::path::PathBuf;

use confit_core::handles::{Route, RouteBase};

use crate::Applier;

impl Applier {
    /// Expands one destination route to its host path.
    pub fn resolve(&self, route: &Route) -> PathBuf {
        self.disk.resolve(route)
    }
}

/// Expands one destination route against host folders.
///
/// Literal carries its path verbatim. Unset homes pass the
/// relative path through intact.
pub(crate) fn resolve_host(route: &Route) -> PathBuf {
    let relative = route.relative();
    match route.base() {
        RouteBase::Literal => relative.to_path_buf(),
        RouteBase::Home => match dirs::home_dir() {
            Some(home) => home.join(relative),
            None => relative.to_path_buf(),
        },
        RouteBase::Config => match dirs::config_dir() {
            Some(base) => base.join(relative),
            None => relative.to_path_buf(),
        },
        RouteBase::Data => match dirs::data_dir() {
            Some(base) => base.join(relative),
            None => relative.to_path_buf(),
        },
        RouteBase::Cache => match dirs::cache_dir() {
            Some(base) => base.join(relative),
            None => relative.to_path_buf(),
        },
    }
}

/// Expands one destination route against fake memory roots.
///
/// Each base maps to a fixed fake folder, so tests seed
/// and assert under stable paths. Literal carries its
/// path verbatim.
pub(crate) fn resolve_memory(route: &Route) -> PathBuf {
    let relative = route.relative();
    match route.base() {
        RouteBase::Home => PathBuf::from("memory-home").join(relative),
        RouteBase::Config => PathBuf::from("memory-config").join(relative),
        RouteBase::Data => PathBuf::from("memory-data").join(relative),
        RouteBase::Cache => PathBuf::from("memory-cache").join(relative),
        RouteBase::Literal => relative.to_path_buf(),
    }
}
