//! Routes
//! Late-bound destination routes.

use std::path::{Path, PathBuf};

use confit_binding::trace::LuaTrace;
use confit_binding_macros::{Face, face_impl};
use confit_core::error::{ContextualResult, DocumentError, Require, contextual};
use confit_driver::paths::BaseDirs;
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
#[derive(Clone, Debug, Face, Serialize, Deserialize)]
#[face(traced, userdata)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// The destination base.
    base: RouteBase,
    /// The relative path.
    relative: PathBuf,
    /// The route declaration stack.
    #[serde(skip)]
    #[face(lua_trace)]
    pub trace: LuaTrace,
}

impl PartialEq for Route {
    /// Report whole route equality over base and relative.
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.relative == other.relative
    }
}

impl Eq for Route {}

impl std::hash::Hash for Route {
    /// Feed base and relative into the hasher.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.base.hash(state);
        self.relative.hash(state);
    }
}

impl PartialOrd for Route {
    /// Report the base and relative order.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Route {
    /// Order routes by base and relative.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.base, &self.relative).cmp(&(other.base, &other.relative))
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
    /// - [`DocumentError::ParseFailed`] for empty paths.
    pub fn new(base: RouteBase, relative: impl Into<PathBuf>) -> ContextualResult<Self> {
        let relative = relative.into();
        if relative.as_os_str().is_empty() {
            return Err(contextual(DocumentError::ParseFailed {
                require: Require::RoutePath,
            }));
        }
        Ok(Self {
            base,
            relative,
            trace: LuaTrace::default(),
        })
    }

    /// Expand one destination route to its host path.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::ParseFailed`] for unresolvable base folders.
    pub fn expand(&self) -> ContextualResult<PathBuf> {
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

    /// Join one member segment onto the route.
    pub fn join(&self, segment: &str) -> Self {
        if segment.is_empty() {
            return self.clone();
        }
        Self {
            base: self.base,
            relative: self.relative.join(segment),
            trace: self.trace.clone(),
        }
    }

    /// Return the relative path.
    pub fn relative_path(&self) -> &Path {
        &self.relative
    }
}

#[face_impl]
impl Route {
    /// Return the destination base.
    ///
    /// # Returns
    ///
    /// - The destination base.
    pub fn base(&self) -> RouteBase {
        self.base
    }

    /// Return the relative path text.
    ///
    /// # Returns
    ///
    /// - The relative path text.
    pub fn relative(&self) -> String {
        self.relative.display().to_string()
    }

    /// Render the portable route display in sugar call form.
    ///
    /// # Returns
    ///
    /// - The sugar call display.
    pub fn display(&self) -> String {
        match self.base {
            RouteBase::Literal => self.relative.display().to_string(),
            _ => format!("{}('{}')", self.base.name(), self.relative.display()),
        }
    }
}

/// Resolve one base folder or fail the run.
///
/// # Errors
///
/// - [`DocumentError::ParseFailed`] for missing base folders.
fn dir_or_fail(_base: RouteBase, found: Option<PathBuf>) -> ContextualResult<PathBuf> {
    found.ok_or_else(|| {
        contextual(DocumentError::ParseFailed {
            require: Require::BaseFolder,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_binding::trace::SourceSite;
    use confit_binding::value::{Decode, Encode};
    use confit_binding_macros::Face;
    use confit_core::error::{DocumentError, ErrorCause};
    use mlua::{Lua, Value};

    /// Foreign handle for route decode tests.
    #[allow(dead_code)]
    #[derive(Clone, Face)]
    #[face(userdata)]
    struct Foreign {
        seed: String,
    }

    #[face_impl]
    impl Foreign {}

    #[test]
    fn route_decode_rejects_table_literals() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("base", "home").unwrap();
        table.set("relative", "a").unwrap();
        assert!(
            Route::decode(&Value::Table(table), &lua).is_err(),
            "table fails"
        );
        let text = lua.create_string("home").unwrap();
        assert!(
            Route::decode(&Value::String(text), &lua).is_err(),
            "string fails"
        );
        assert!(Route::decode(&Value::Nil, &lua).is_err(), "nil fails");
        let foreign = lua
            .create_userdata(Foreign {
                seed: "base".to_owned(),
            })
            .unwrap();
        assert!(
            Route::decode(&Value::UserData(foreign), &lua).is_err(),
            "foreign userdata fails"
        );
    }

    #[test]
    fn traced_outside_lua_holds_empty_trace() {
        let lua = Lua::new();
        let traced = Route::traced(&lua, Route::new(RouteBase::Home, "same").unwrap());
        assert!(traced.trace.0.is_empty(), "bare state holds no frames");
        assert_eq!(traced.relative(), "same");
    }

    #[test]
    fn traced_routes_match_over_base_and_relative() {
        let lua = Lua::new();
        let first = Route::traced(&lua, Route::new(RouteBase::Home, "same").unwrap());
        let mut second = first.clone();
        second.trace = LuaTrace(vec![SourceSite {
            file: "tool.lua".to_owned(),
            line: "3".to_owned(),
        }]);
        assert_eq!(first, second);
        assert_eq!(first.cmp(&second), std::cmp::Ordering::Equal);
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut left = DefaultHasher::new();
        first.hash(&mut left);
        let mut right = DefaultHasher::new();
        second.hash(&mut right);
        assert_eq!(left.finish(), right.finish());
    }

    #[test]
    fn route_methods_run_through_lua() {
        let lua = Lua::new();
        let route = Route::traced(&lua, Route::new(RouteBase::Home, ".bashrc").unwrap());
        let encoded = route.encode(&lua).unwrap();
        lua.globals().set("route_case", encoded).unwrap();
        let display: String = lua.load("return route_case:display()").eval().unwrap();
        assert_eq!(display, "home('.bashrc')");
        let base: String = lua.load("return route_case:base()").eval().unwrap();
        assert_eq!(base, "home");
        let relative: String = lua.load("return route_case:relative()").eval().unwrap();
        assert_eq!(relative, ".bashrc");
        assert_eq!(route.join("sub").display(), "home('.bashrc/sub')");
    }

    #[test]
    fn serde_keeps_table_shape_without_trace() {
        let route = Route::new(RouteBase::Home, "a/b").unwrap();
        let text = serde_json::to_string(&route).unwrap();
        assert_eq!(text, r#"{"base":"home","relative":"a/b"}"#);
        let back: Route = serde_json::from_str(&text).unwrap();
        assert_eq!(back, route);
        assert!(back.trace.0.is_empty(), "decoded trace stays empty");
    }

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
            assert_eq!(route.relative_path(), Path::new("starship/starship.toml"));
        }
    }

    #[test]
    fn route_rejects_empty_path() {
        let error = match Route::new(RouteBase::Home, "") {
            Ok(_) => panic!("empty route passes"),
            Err(error) => error,
        };
        assert!(matches!(
            error.cause(),
            ErrorCause::DocumentError(DocumentError::ParseFailed { .. })
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
