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
/// Base plus relative path.
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
    /// Empty paths fail as parse errors. Validity beyond non-empty
    /// stays apply-time business.
    ///
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
    /// Unknown bases, missing separators, and empty paths
    /// fail as parse errors.
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
}
