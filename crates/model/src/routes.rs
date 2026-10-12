//! Routes
//! Late-bound destination routes.

use std::path::{Path, PathBuf};

use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use mlua::{Lua, Value};
use schemars::JsonSchema;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// Portable standard folders enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Face, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(rename_all = "lowercase")]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Face, JsonSchema)]
#[face(table, validate = check_route)]
pub struct Route {
    /// The destination base.
    base: RouteBase,
    /// The relative path.
    #[face(proxy = String, convert = decode_relative)]
    relative: PathBuf,
}

/// One jailed segment offense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Face)]
#[face(table, string)]
pub enum SegmentFault {
    /// Empty segment string.
    Empty,
    /// Absolute root where relatives belong.
    Absolute,
    /// Parent step climbing out.
    Parent,
    /// Drive prefix showing a foreign host.
    Drive,
    /// Backslash separator from another platform.
    Backslash,
    /// Empty component from doubled or trailing slashes.
    EmptyComponent,
}

impl SegmentFault {
    /// Return the jail reason for the fault.
    pub fn reason(&self) -> &'static str {
        match self {
            Self::Empty => "holds no path",
            Self::Absolute => "must stay relative",
            Self::Parent => "parent steps escape",
            Self::Drive => "drive prefixes escape",
            Self::Backslash => "backslashes escape",
            Self::EmptyComponent => "empty components escape",
        }
    }
}

impl std::fmt::Display for SegmentFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

/// Split one segment string into checked components.
///
/// # Errors
///
/// - [`SegmentFault`] for every escaping or empty shape.
pub fn split_checked(
    raw: &str,
    absolute_ok: bool,
) -> std::result::Result<Vec<String>, SegmentFault> {
    if raw.contains('\\') {
        return Err(SegmentFault::Backslash);
    }
    if raw.len() >= 2 && raw.as_bytes()[0].is_ascii_alphabetic() && raw.as_bytes()[1] == b':' {
        return Err(SegmentFault::Drive);
    }
    let absolute = raw.starts_with('/');
    if absolute && !absolute_ok {
        return Err(SegmentFault::Absolute);
    }
    let mut parts = Vec::new();
    for component in raw.trim_start_matches('/').split('/') {
        if component.is_empty() {
            return Err(SegmentFault::EmptyComponent);
        }
        if component == ".." {
            return Err(SegmentFault::Parent);
        }
        parts.push(component.to_owned());
    }
    if parts.is_empty() {
        return Err(SegmentFault::Empty);
    }
    Ok(parts)
}

impl<'de> Deserialize<'de> for RouteBase {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(BaseVisitor)
    }
}

struct BaseVisitor;

impl Visitor<'_> for BaseVisitor {
    type Value = RouteBase;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("known base 'home', 'config', 'data', 'cache', or 'literal'")
    }

    fn visit_str<E>(self, name: &str) -> std::result::Result<RouteBase, E>
    where
        E: de::Error,
    {
        match name {
            "home" => Ok(RouteBase::Home),
            "config" => Ok(RouteBase::Config),
            "data" => Ok(RouteBase::Data),
            "cache" => Ok(RouteBase::Cache),
            "literal" => Ok(RouteBase::Literal),
            _ => Err(E::custom(
                [
                    "unknown base '",
                    name,
                    "': want known base 'home', 'config', 'data', 'cache', or 'literal'",
                ]
                .concat(),
            )),
        }
    }

    fn visit_string<E>(self, name: String) -> std::result::Result<RouteBase, E>
    where
        E: de::Error,
    {
        self.visit_str(&name)
    }
}

impl<'de> Deserialize<'de> for Route {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        deserializer.deserialize_any(RouteVisitor)
    }
}

struct RouteVisitor;

impl<'de> Visitor<'de> for RouteVisitor {
    type Value = Route;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a route table")
    }

    fn visit_str<E>(self, text: &str) -> std::result::Result<Route, E>
    where
        E: de::Error,
    {
        Err(E::invalid_type(de::Unexpected::Str(text), &self))
    }

    fn visit_string<E>(self, text: String) -> std::result::Result<Route, E>
    where
        E: de::Error,
    {
        self.visit_str(&text)
    }

    fn visit_map<A>(self, mut access: A) -> std::result::Result<Route, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut base = None;
        let mut relative = None;
        while let Some(key) = access.next_key::<String>()? {
            match key.as_str() {
                "base" => {
                    if base.is_some() {
                        return Err(de::Error::duplicate_field("base"));
                    }
                    base = Some(access.next_value::<RouteBase>()?);
                }
                "relative" => {
                    if relative.is_some() {
                        return Err(de::Error::duplicate_field("relative"));
                    }
                    relative = Some(access.next_value::<String>()?);
                }
                _ => return Err(de::Error::unknown_field(&key, &["base", "relative"])),
            }
        }
        let base = base.ok_or_else(|| de::Error::missing_field("base"))?;
        let relative: String = relative.ok_or_else(|| de::Error::missing_field("relative"))?;
        check_relative(base, &relative).map_err(de::Error::custom)
    }
}

/// Build one route from its base with its raw relative text.
///
/// # Errors
///
/// - [`Error::Segment`] for escaping or empty shapes.
/// - [`Error::Parse`] for empty paths.
fn check_relative(base: RouteBase, relative: &str) -> Result<Route> {
    let absolute_ok = base == RouteBase::Literal;
    match split_checked(relative, absolute_ok) {
        Ok(parts) => {
            let joined = parts.join("/");
            let shaped = if absolute_ok && relative.starts_with('/') {
                format!("/{joined}")
            } else {
                joined
            };
            Route::new(base, shaped)
        }
        Err(fault) => Err(Error::Segment {
            input: relative.to_owned(),
            fault,
        }),
    }
}

/// Decode one relative string into its path.
///
/// # Errors
///
/// - [`Mismatch`] for values outside the string shape.
fn decode_relative(lua: &Lua, value: &Value) -> std::result::Result<PathBuf, Mismatch> {
    String::decode(value, lua).map(PathBuf::from)
}

/// Check one route keeps its relative path inside its base jail.
///
/// # Errors
///
/// - [`mlua::Error`] for escaping or empty shapes.
fn check_route(route: &Route) -> mlua::Result<()> {
    let absolute_ok = route.base == RouteBase::Literal;
    let text = route.relative.display().to_string();
    match split_checked(&text, absolute_ok) {
        Ok(_) => Ok(()),
        Err(fault) => Err(mlua::Error::external(Error::Segment { input: text, fault })),
    }
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

    /// Join one member segment onto the route.
    ///
    /// - Empty segments keep the route unchanged.
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
    /// - Literal carries its path unchanged.
    /// - Unset homes pass the relative path through intact.
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
    fn expand_carries_literal_unchanged() {
        for path in ["/opt/confit/tool", "relative/tool"] {
            let route = Route::new(RouteBase::Literal, path).unwrap();
            assert_eq!(route.expand(), Path::new(path));
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

    #[test]
    fn decode_keeps_literal_leading_slashes() {
        let value = serde_json::json!({ "base": "literal", "relative": "/opt/x" });
        match serde_json::from_value::<Route>(value) {
            Ok(route) => {
                assert_eq!(route.relative(), Path::new("/opt/x"));
                assert_eq!(route.display(), "/opt/x");
            }
            Err(error) => panic!("literal slash decodes: {error}"),
        }
    }
}
