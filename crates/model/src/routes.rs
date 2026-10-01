//! Routes
//!
//! Late-bound destination routes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Destination base for a route.
///
/// Home, config, data, and cache resolve on the applying host.
/// Literal carries a host-specific path verbatim.
/// Nothing resolves here; resolution is apply-time business.
///
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteBase {
    /// User home folder.
    Home,
    /// Platform config folder.
    Config,
    /// Platform data folder.
    Data,
    /// Platform cache folder.
    Cache,
    /// Host-specific literal path.
    Literal,
}

/// Late-bound destination route.
///
/// Base and relative path.
/// The path holds a non-empty shape only.
/// Parents, permissions, and platform validity resolve at apply time.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Route {
    base: RouteBase,
    relative: PathBuf,
}

impl RouteBase {
    /// Reads the lowercase base name.
    ///
    /// # Returns
    ///
    /// The base name for route display and log lines.
    ///
    pub fn name(&self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Config => "config",
            Self::Data => "data",
            Self::Cache => "cache",
            Self::Literal => "literal",
        }
    }
}

impl Route {
    /// Builds a destination route from a base and a path.
    ///
    /// # Errors
    ///
    /// - [`Error::Parse`] for empty paths.
    pub fn new(base: RouteBase, relative: impl Into<PathBuf>) -> Result<Self> {
        let relative = relative.into();
        if relative.as_os_str().is_empty() {
            return Err(Error::Parse {
                input: relative.display().to_string(),
                want: "non-empty route path".to_owned(),
            });
        }
        Ok(Self { base, relative })
    }

    /// Joins one member segment onto the route.
    ///
    /// Empty segments keep the route unchanged, so joined
    /// member paths never fail.
    ///
    pub fn join(&self, segment: &str) -> Self {
        if segment.is_empty() {
            return self.clone();
        }
        Self {
            base: self.base,
            relative: self.relative.join(segment),
        }
    }

    /// Renders the portable route display.
    ///
    /// # Returns
    ///
    /// The `base:relative` text for plan keys and drift lines.
    ///
    pub fn display(&self) -> String {
        format!("{}:{}", self.base.name(), self.relative.display())
    }

    /// Parses one portable route display.
    ///
    /// # Arguments
    ///
    /// * `text` - the `base:relative` text under parsing.
    ///
    /// # Returns
    ///
    /// The route for known bases with a non-empty relative path.
    ///
    /// # Errors
    ///
    /// - [`Error::Parse`] for unknown bases, missing separators, and empty paths.
    ///
    pub fn parse(text: &str) -> Result<Self> {
        let Some((base_name, relative)) = text.split_once(':') else {
            return Err(Error::Parse {
                input: text.to_owned(),
                want: "'base:relative' like 'home:.bashrc'".to_owned(),
            });
        };
        let base = match base_name {
            "home" => RouteBase::Home,
            "config" => RouteBase::Config,
            "data" => RouteBase::Data,
            "cache" => RouteBase::Cache,
            "literal" => RouteBase::Literal,
            _ => {
                return Err(Error::Parse {
                    input: text.to_owned(),
                    want: "known base 'home', 'config', 'data', 'cache', or 'literal'".to_owned(),
                });
            }
        };
        Self::new(base, relative).map_err(|_| Error::Parse {
            input: text.to_owned(),
            want: "'base:relative' like 'home:.bashrc'".to_owned(),
        })
    }

    /// Reads the destination base.
    ///
    pub fn base(&self) -> RouteBase {
        self.base
    }

    /// Reads the relative path.
    ///
    pub fn relative(&self) -> &Path {
        &self.relative
    }

    /// Expands one destination route to its host path.
    ///
    /// Literal carries its path verbatim. Unset homes pass
    /// the relative path through intact.
    pub fn expand(&self) -> PathBuf {
        match self.base {
            RouteBase::Literal => self.relative.clone(),
            RouteBase::Home => match dirs::home_dir() {
                Some(home) => home.join(&self.relative),
                None => self.relative.clone(),
            },
            RouteBase::Config => match dirs::config_dir() {
                Some(base) => base.join(&self.relative),
                None => self.relative.clone(),
            },
            RouteBase::Data => match dirs::data_dir() {
                Some(base) => base.join(&self.relative),
                None => self.relative.clone(),
            },
            RouteBase::Cache => match dirs::cache_dir() {
                Some(base) => base.join(&self.relative),
                None => self.relative.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    #[test]
    fn route_builds_for_every_base() {
        for base in [
            RouteBase::Home,
            RouteBase::Config,
            RouteBase::Data,
            RouteBase::Cache,
            RouteBase::Literal,
        ] {
            let route = Route::new(base, "starship/starship.toml").unwrap();
            assert_eq!(route.base(), base);
            assert_eq!(route.relative(), Path::new("starship/starship.toml"));
        }
    }

    #[test]
    fn route_rejects_empty_path() {
        let error = match Route::new(RouteBase::Home, "") {
            Ok(_) => panic!("empty route passes"),
            Err(error) => error,
        };
        assert!(matches!(error, Error::Parse { .. }));
    }

    #[test]
    fn expand_matches_host_mapping_for_every_base() {
        let relative = Path::new("starship/starship.toml");
        let cases = [
            (RouteBase::Home, dirs::home_dir()),
            (RouteBase::Config, dirs::config_dir()),
            (RouteBase::Data, dirs::data_dir()),
            (RouteBase::Cache, dirs::cache_dir()),
        ];
        for (base, found) in cases {
            let route = Route::new(base, relative).unwrap();
            let want = match found {
                Some(home) => home.join(relative),
                None => relative.to_path_buf(),
            };
            assert_eq!(route.expand(), want);
        }
    }

    #[test]
    fn expand_carries_literal_verbatim() {
        for path in ["/opt/confit/tool", "relative/tool"] {
            let route = Route::new(RouteBase::Literal, path).unwrap();
            assert_eq!(route.expand(), Path::new(path));
        }
    }
}
