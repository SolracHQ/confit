//! Route expansion against host folders.

use std::path::PathBuf;

use confit_model::handles::{Route, RouteBase};

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
