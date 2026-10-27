//! Routes
//! Late-bound destination routes.

use std::path::{Path, PathBuf};

use confit_binding::fault::FaceFault;
use confit_binding::value::Decode;
use confit_binding_macros::Face;
use confit_core::error::{DocumentError, ErrorContext, Require, Result, fail};
use confit_driver::paths::BaseDirs;
use mlua::{Lua, Value};
use serde::{Deserialize, Serialize};

/// Portable standard folders enum.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Face,
)]
#[serde(rename_all = "lowercase")]
#[face(table, string)]
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

/// Portable route over standard folders.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Face)]
#[serde(deny_unknown_fields)]
#[face(table)]
pub struct Route {
    /// The destination base.
    base: RouteBase,
    /// The relative path.
    #[face(proxy = String, decode_hook = decode_relative)]
    relative: PathBuf,
}

/// Decode one relative string into its path.
///
/// # Errors
///
/// - [`FaceFault`] for values outside the string shape.
fn decode_relative(lua: &Lua, value: &Value) -> std::result::Result<PathBuf, FaceFault> {
    String::decode(value, lua).map(PathBuf::from)
}

impl RouteBase {
    /// Return the lowercase base name.
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
    /// Build a destination route from a base and a path.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::ParseFailed`] for empty paths.
    pub fn new(base: RouteBase, relative: impl Into<PathBuf>) -> Result<Self> {
        let relative = relative.into();
        if relative.as_os_str().is_empty() {
            return fail(DocumentError::ParseFailed {
                input: relative.display().to_string(),
                require: Require::RoutePath,
            });
        }
        Ok(Self { base, relative })
    }

    /// Join one member segment onto the route.
    pub fn join(&self, segment: &str) -> Self {
        if segment.is_empty() {
            return self.clone();
        }
        Self {
            base: self.base,
            relative: self.relative.join(segment),
        }
    }

    /// Render the portable route display in sugar call form.
    pub fn display(&self) -> String {
        match self.base {
            RouteBase::Literal => self.relative.display().to_string(),
            _ => format!("{}('{}')", self.base.name(), self.relative.display()),
        }
    }

    /// Return the destination base.
    pub fn base(&self) -> RouteBase {
        self.base
    }

    /// Return the relative path.
    pub fn relative(&self) -> &Path {
        &self.relative
    }

    /// Expand one destination route to its host path.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::ParseFailed`] for unresolvable base folders.
    pub fn expand(&self) -> Result<PathBuf> {
        let bases = BaseDirs::new();
        let base = match self.base {
            RouteBase::Literal => return Ok(self.relative.clone()),
            RouteBase::Home => dir_or_fail(RouteBase::Home, bases.home)?,
            RouteBase::Config => dir_or_fail(RouteBase::Config, bases.config)?,
            RouteBase::Data => dir_or_fail(RouteBase::Data, bases.data)?,
            RouteBase::Cache => dir_or_fail(RouteBase::Cache, bases.cache)?,
        };
        Ok(base.join(&self.relative))
    }
}

/// Resolve one base folder or fail the run.
///
/// # Errors
///
/// - [`DocumentError::ParseFailed`] for missing base folders.
fn dir_or_fail(base: RouteBase, found: Option<PathBuf>) -> Result<PathBuf> {
    found.ok_or_else(|| {
        ErrorContext::from(DocumentError::ParseFailed {
            input: base.name().to_owned(),
            require: Require::BaseFolder,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::error::{DocumentError, ErrorCause, ErrorContext};

    #[test]
    fn route_built_for_every_base() {
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
        assert!(matches!(
            error,
            ErrorContext::Leaf(ErrorCause::DocumentError(DocumentError::ParseFailed { .. }))
        ));
    }

    #[test]
    fn expand_joins_each_base_under_shared_root() {
        let relative = Path::new("starship/starship.toml");
        let bases = BaseDirs::new();
        let cases = [
            (RouteBase::Home, bases.home.clone()),
            (RouteBase::Config, bases.config.clone()),
            (RouteBase::Data, bases.data.clone()),
            (RouteBase::Cache, bases.cache.clone()),
        ];
        for (base, found) in cases {
            let route = Route::new(base, relative).unwrap();
            match found {
                Some(root) => assert_eq!(route.expand().unwrap(), root.join(relative)),
                None => assert!(route.expand().is_err()),
            }
        }
    }

    #[test]
    fn expand_carries_literal_unchanged() {
        for path in ["/opt/confit/tool", "relative/tool"] {
            let route = Route::new(RouteBase::Literal, path).unwrap();
            assert_eq!(route.expand().unwrap(), Path::new(path));
        }
    }

    #[test]
    fn display_renders_sugar_calls_with_bare_literals() {
        let home = Route::new(RouteBase::Home, ".bashrc").unwrap();
        assert_eq!(home.display(), "home('.bashrc')");
        let config = Route::new(RouteBase::Config, "a/b").unwrap();
        assert_eq!(config.display(), "config('a/b')");
        let literal = Route::new(RouteBase::Literal, "/opt/x").unwrap();
        assert_eq!(literal.display(), "/opt/x");
    }
}
