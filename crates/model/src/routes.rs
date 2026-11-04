//! Late-bound destination routes.

use std::path::{Path, PathBuf};

use confit_core::error::{ContextualResult, DocumentError, Require, contextual};
use confit_driver::paths::BaseDirs;
use lua_binding::trace::LuaTrace;
use lua_binding::{Shape, lua_methods};
use serde::{Deserialize, Serialize};

/// A standard userspace folder.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Shape,
)]
#[serde(rename_all = "lowercase")]
#[shape(table, str_alias)]
pub enum RouteBase {
    /// The platform cache folder.
    Cache,
    /// The platform config folder.
    Config,
    /// The platform data folder.
    Data,
    /// The user home folder.
    Home,
    /// A host path taken verbatim.
    Literal,
}

/// A destination route.
#[derive(Clone, Debug, Shape, Serialize, Deserialize)]
#[shape(traced, object)]
#[serde(deny_unknown_fields)]
pub struct Route {
    /// The destination base.
    #[shape(get)]
    base: RouteBase,
    /// The relative path.
    #[shape(get)]
    relative: PathBuf,
    /// The route declaration stack.
    #[serde(skip)]
    #[shape(lua_trace)]
    pub trace: LuaTrace,
}

impl PartialEq for Route {
    /// Returns whether the base and the relative path match.
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.relative == other.relative
    }
}

impl Eq for Route {}

impl std::hash::Hash for Route {
    /// Hashes the base and the relative path, and skips the trace.
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.base.hash(state);
        self.relative.hash(state);
    }
}

impl PartialOrd for Route {
    /// Returns the order of the two routes by base and relative path.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Route {
    /// Orders routes by base first, then the relative path.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.base, &self.relative).cmp(&(other.base, &other.relative))
    }
}

impl RouteBase {
    /// Returns the base name as text in lowercase.
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
    /// Builds a destination route that resolves under a base folder.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::ParseFailed`] when the path is empty.
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

    /// Resolves the route under its base folder into a host path.
    ///
    /// # Errors
    ///
    /// - [`DocumentError::ParseFailed`] when a base folder is missing.
    pub fn expand(&self) -> ContextualResult<PathBuf> {
        let bases = BaseDirs::new();
        let base = match self.base {
            RouteBase::Literal => return Ok(self.relative.clone()),
            RouteBase::Home => require_base_folder(RouteBase::Home, bases.home)?,
            RouteBase::Config => require_base_folder(RouteBase::Config, bases.config)?,
            RouteBase::Data => require_base_folder(RouteBase::Data, bases.data)?,
            RouteBase::Cache => require_base_folder(RouteBase::Cache, bases.cache)?,
        };
        Ok(base.join(&self.relative))
    }

    /// Returns a route with one path segment appended.
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

    /// Returns the path below the base folder.
    pub fn relative_path(&self) -> &Path {
        &self.relative
    }

    /// Returns the base folder that anchors the route.
    pub fn base(&self) -> RouteBase {
        self.base
    }

    /// Returns the path below the base folder as display text.
    pub fn relative(&self) -> String {
        self.relative.display().to_string()
    }
}

#[lua_methods]
impl Route {
    /// Returns the route as a sugar call like `home('.bashrc')`.
    ///
    /// # Returns
    ///
    /// - The call text for the route.
    pub fn display(&self) -> String {
        match self.base {
            RouteBase::Literal => self.relative.display().to_string(),
            _ => format!("{}('{}')", self.base.name(), self.relative.display()),
        }
    }
}

/// Returns the base folder, or a parse failure when it is absent.
///
/// # Errors
///
/// - [`DocumentError::ParseFailed`] when the base folder is missing.
fn require_base_folder(_base: RouteBase, found: Option<PathBuf>) -> ContextualResult<PathBuf> {
    found.ok_or_else(|| {
        contextual(DocumentError::ParseFailed {
            require: Require::BaseFolder,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_core::error::{DocumentError, ErrorCause};
    use lua_binding::Shape;
    use lua_binding::trace::SourceSite;
    use lua_binding::value::{Decode, Encode};
    use mlua::{Lua, Value};

    /// A second userdata shape for decode tests.
    #[allow(dead_code)]
    #[derive(Clone, Shape)]
    #[shape(object)]
    struct OtherShape {
        seed: String,
    }

    #[lua_methods]
    impl OtherShape {}

    #[test]
    fn route_decode_rejects_table_literals() {
        let lua = Lua::new();
        let table = lua.create_table().unwrap();
        table.set("base", "home").unwrap();
        table.set("relative", "a").unwrap();
        assert!(
            Route::decode(&Value::Table(table), &lua).is_err(),
            "the route decoder rejects a table"
        );
        let text = lua.create_string("home").unwrap();
        assert!(
            Route::decode(&Value::String(text), &lua).is_err(),
            "the route decoder rejects a string"
        );
        assert!(
            Route::decode(&Value::Nil, &lua).is_err(),
            "the route decoder rejects nil"
        );
        let foreign = lua
            .create_userdata(OtherShape {
                seed: "base".to_owned(),
            })
            .unwrap();
        assert!(
            Route::decode(&Value::UserData(foreign), &lua).is_err(),
            "the route decoder rejects other userdata"
        );
    }

    #[test]
    fn traced_route_starts_with_an_empty_trace() {
        let lua = Lua::new();
        let traced = Route::traced(&lua, Route::new(RouteBase::Home, "same").unwrap());
        assert!(
            traced.trace.0.is_empty(),
            "the fresh route holds an empty trace"
        );
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
        let base: String = lua.load("return route_case.base").eval().unwrap();
        assert_eq!(base, "home");
        let relative: String = lua.load("return route_case.relative").eval().unwrap();
        assert_eq!(relative, ".bashrc");
        assert_eq!(route.join("sub").display(), "home('.bashrc/sub')");
    }

    #[test]
    fn serde_keeps_the_table_shape_with_an_empty_trace() {
        let route = Route::new(RouteBase::Home, "a/b").unwrap();
        let text = serde_json::to_string(&route).unwrap();
        assert_eq!(text, r#"{"base":"home","relative":"a/b"}"#);
        let back: Route = serde_json::from_str(&text).unwrap();
        assert_eq!(back, route);
        assert!(
            back.trace.0.is_empty(),
            "the decoded route holds an empty trace"
        );
    }

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
            assert_eq!(route.relative_path(), Path::new("starship/starship.toml"));
        }
    }

    #[test]
    fn route_rejects_empty_path() {
        let error = match Route::new(RouteBase::Home, "") {
            Ok(_) => panic!("the route builder accepts an empty path"),
            Err(error) => error,
        };
        assert!(matches!(
            error.cause(),
            ErrorCause::DocumentError(DocumentError::ParseFailed { .. })
        ));
    }

    #[test]
    fn expand_joins_the_relative_path_to_each_base() {
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
    fn expand_returns_the_literal_path() {
        for path in ["/opt/confit/tool", "relative/tool"] {
            let route = Route::new(RouteBase::Literal, path).unwrap();
            assert_eq!(route.expand().unwrap(), Path::new(path));
        }
    }

    #[test]
    fn display_renders_sugar_calls_and_bare_literals() {
        let home = Route::new(RouteBase::Home, ".bashrc").unwrap();
        assert_eq!(home.display(), "home('.bashrc')");
        let config = Route::new(RouteBase::Config, "a/b").unwrap();
        assert_eq!(config.display(), "config('a/b')");
        let literal = Route::new(RouteBase::Literal, "/opt/x").unwrap();
        assert_eq!(literal.display(), "/opt/x");
    }
}
