//! Paths
//!
//! Path values over host bases with literal verbs. Every component
//! checks at the call, escapes fail where written.

use mlua::{FromLua, FromLuaMulti, Lua, MultiValue, Value};
use mlua_extras::TypedUserData;
use mlua_extras::typed::generator::DefinitionBuilder;
use mlua_extras::typed::{Type, Typed};
use mlua_extras::typeduserdata_impl;

use confit_model::routes::{Route, RouteBase};
use confit_surface_face::mismatch::received;
use confit_surface_face::module::{Module, VerbDecl, readonly};

use crate::error::ApiError;

/// Path value for late-bound destinations.
#[derive(Debug, Clone, TypedUserData)]
pub struct PathHandle {
    /// The destination route.
    #[lua(skip)]
    route: Route,
}

impl PathHandle {
    /// Builds one path value from its route.
    pub fn new(route: Route) -> Self {
        Self { route }
    }

    /// Reads the destination route.
    pub fn route(&self) -> &Route {
        &self.route
    }
}

impl FromLua for PathHandle {
    fn from_lua(value: Value, _lua: &Lua) -> mlua::Result<Self> {
        match value {
            Value::UserData(data) => match data.borrow::<Self>() {
                Ok(path) => Ok(path.clone()),
                Err(_) => Err(path_mismatch(&Value::UserData(data.clone()))),
            },
            _ => Err(path_mismatch(&value)),
        }
    }
}

fn path_mismatch(value: &Value) -> mlua::Error {
    ApiError::Mismatch {
        slot: "path".to_owned(),
        want: "a confit.path value".to_owned(),
        received: received(value).to_owned(),
    }
    .into()
}

/// Path segments as strings.
struct Segments(Vec<String>);

impl Typed for Segments {
    fn ty() -> Type {
        Type::string()
    }

    fn is_variadic() -> bool {
        true
    }
}

impl FromLuaMulti for Segments {
    fn from_lua_multi(mut values: MultiValue, _lua: &Lua) -> mlua::Result<Self> {
        let mut raws = Vec::new();
        for (position, value) in values.drain(..).enumerate() {
            match value {
                Value::String(text) => raws.push(text.to_string_lossy()),
                _ => {
                    let slot = format!("segment {}", position + 1);
                    return Err(ApiError::Mismatch {
                        slot,
                        want: "a string".to_owned(),
                        received: received(&value).to_owned(),
                    }
                    .into());
                }
            }
        }
        Ok(Self(raws))
    }
}

#[typeduserdata_impl]
impl PathHandle {
    /// Reads the destination base name.
    fn base(&self) -> mlua::Result<String> {
        Ok(self.route.base().name().to_owned())
    }

    /// Reads the portable route display.
    fn display(&self) -> mlua::Result<String> {
        Ok(self.route.display())
    }

    /// Reads the relative path text.
    fn relative(&self) -> mlua::Result<String> {
        Ok(self.route.relative().display().to_string())
    }
}

/// One segment fault holding the offense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentFault {
    /// Empty segment string.
    Empty,
    /// Absolute root where relatives belong.
    Absolute,
    /// Parent step climbing out.
    Parent,
    /// Drive prefix naming a foreign host.
    Drive,
    /// Backslash separator from another platform.
    Backslash,
    /// Empty component from doubled or trailing slashes.
    EmptyComponent,
}

impl SegmentFault {
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

/// Splits one segment string into checked components.
///
/// Slashes separate, dots pass through.
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

/// Builds the path module table with its face.
pub fn build(lua: &Lua) -> mlua::Result<(Value, DefinitionBuilder)> {
    let mut module = Module::start(lua, "Late-bound destination routes over host bases.")?;
    module.register::<PathHandle>();
    for (verb, doc, ret, base) in [
        (
            "home",
            "Joins the user home folder with segments.",
            "The joined path value.",
            RouteBase::Home,
        ),
        (
            "config",
            "Joins the platform config folder with segments.",
            "The joined path value.",
            RouteBase::Config,
        ),
        (
            "data",
            "Joins the platform data folder with segments.",
            "The joined path value.",
            RouteBase::Data,
        ),
        (
            "cache",
            "Joins the platform cache folder with segments.",
            "The joined path value.",
            RouteBase::Cache,
        ),
        (
            "literal",
            "Carries a host-specific path verbatim.",
            "The literal path value.",
            RouteBase::Literal,
        ),
    ] {
        module.verb::<Segments, PathHandle, _>(
            lua,
            VerbDecl {
                name: verb,
                doc,
                params: &[("...", "Path segments.")],
                rets: &[ret],
            },
            move |_, args| base_impl(verb, base, args),
        )?;
    }
    let (table, face) = module.finish();
    Ok((readonly(lua, "confit.path", table)?, face))
}

/// Builds one destination route from segment names under a base.
///
/// # Errors
///
/// - [`ApiError::EmptySegments`] for missing segments.
/// - [`ApiError::Escape`] for escaping components.
/// - [`ApiError::Model`] for route build failures.
fn base_impl(name: &'static str, base: RouteBase, args: Segments) -> mlua::Result<PathHandle> {
    let caller = format!("confit.path.{name}");
    let Segments(raws) = args;
    if raws.is_empty() {
        return Err(ApiError::EmptySegments {
            caller: caller.clone(),
        }
        .into());
    }
    let mut parts = Vec::new();
    for raw in &raws {
        split_checked(raw, base == RouteBase::Literal)
            .map_err(|fault| ApiError::Escape {
                caller: caller.clone(),
                input: raw.clone(),
                reason: fault.reason().to_owned(),
            })?
            .into_iter()
            .for_each(|part| parts.push(part));
    }
    Ok(Route::new(base, parts.join("/"))
        .map(PathHandle::new)
        .map_err(ApiError::from)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use confit_driver::fs::TestGuard;
    use confit_store::{StoreRoots, Stores};

    fn test_stores(dir: &Path) -> Stores {
        let (sender, _) = crossbeam_channel::unbounded();
        Stores::new(
            StoreRoots {
                config_base: dir.join("config"),
                cache_base: dir.join("cache"),
                temp_base: dir.join("temp"),
                project_root: dir.to_path_buf(),
            },
            sender,
        )
    }

    fn test_lua(stores: &Stores) -> Lua {
        let lua = Lua::new();
        let _ = lua.set_app_data(stores.clone());
        let path = crate::api::builtin(&lua, "confit.path").unwrap();
        lua.globals().set("path", path).unwrap();
        lua
    }

    fn run(lua: &Lua, source: &str) -> mlua::Result<Value> {
        lua.load(source).call(())
    }

    fn display_of(value: Value) -> String {
        match value {
            Value::String(found) => found.to_string_lossy(),
            _ => panic!("path display answers a string"),
        }
    }

    #[test]
    fn verbs_join_segments_into_values() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            local home = path.home(".bashrc")
            return home:display() .. "|" .. home:base() .. "|" .. home:relative()
        "#;
        match run(&lua, source) {
            Ok(value) => assert_eq!(display_of(value), "home:.bashrc|home|.bashrc"),
            Err(error) => panic!("path verbs join: {error}"),
        }
    }

    #[test]
    fn escapes_fail_where_written() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let cases = [
            ("../outside", "parent steps"),
            ("/etc/passwd", "must stay relative"),
            ("C:/tool", "drive prefixes"),
            ("back\\slash", "backslashes"),
            ("a//b", "empty components"),
            ("trailing/", "empty components"),
        ];
        for (segment, reason) in cases {
            let source = format!("return path.home({segment:?})");
            match run(&lua, &source) {
                Ok(_) => panic!("escaping segment passes: {segment}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(
                        text.contains("confit.path.home"),
                        "names the caller: {text}"
                    );
                    assert!(text.contains(reason), "names the escape: {text}");
                }
            }
        }
    }

    #[test]
    fn missing_with_foreign_segments_fail() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        match run(&lua, "return path.home()") {
            Ok(_) => panic!("empty segments pass"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("one path segment"), "names the want: {text}");
            }
        }
        match run(&lua, "return path.home(1)") {
            Ok(_) => panic!("numeric segment passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("segment 1"), "names the position: {text}");
                assert!(text.contains("a string"), "names the want: {text}");
                assert!(text.contains("number"), "names the received: {text}");
            }
        }
    }

    #[test]
    fn literal_carries_absolute_but_refuses_parents() {
        let dir = tempfile::tempdir().unwrap();
        let _guard = TestGuard::install();
        let stores = test_stores(dir.path());
        let lua = test_lua(&stores);
        let source = r#"
            return path.literal("/opt/legacy/app.conf"):display()
        "#;
        match run(&lua, source) {
            Ok(value) => assert!(
                display_of(value).contains("opt/legacy/app.conf"),
                "literal carries the host path"
            ),
            Err(error) => panic!("literal carries absolute: {error}"),
        }
        let source = "return path.literal(\"../out\")";
        match run(&lua, source) {
            Ok(_) => panic!("literal parent passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("parent steps"), "names the escape: {text}");
            }
        }
    }
}
