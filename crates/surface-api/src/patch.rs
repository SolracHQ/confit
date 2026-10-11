//! Patches
//! Patch declarations.

use mlua::{FromLua, Function, Lua, Table, Value};
use mlua_extras::typed::Typed;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use confit_model::document::StructuredFormat;
use confit_model::routes::Route;
use confit_surface_face::callback::Callback;
use confit_surface_face::mismatch::received;
use confit_surface_face::module::Named;

use crate::document::{RcDocument, StructuredDocument, decode_named_shape};
use crate::error::ApiError;
use crate::error::api_of;
use crate::paths::RouteMarker;

confit_surface_face::function_face! {
    /// Face for patch apply callbacks.
    pub struct ApplyArgs<Document> {
        /// Target route of the patched document.
        route: RouteMarker,
        /// Live document under patching.
        document: Document,
    }
}

confit_surface_face::function_face! {
    /// Face for patch filter callbacks.
    pub struct FilterArgs {
        /// Target route of the candidate document.
        route: RouteMarker,
    }
}

/// Patch merge level.
///
/// - Ranks run 0-4 from Minor to Major.
/// - Names parse case-insensitively.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// Lowest merge level.
    Minor,
    /// Low merge level.
    Low,
    /// Default merge level.
    #[default]
    Normal,
    /// High merge level.
    High,
    /// Highest merge level.
    Major,
}

confit_surface_face::typed!(Level);

impl Level {
    /// Return the numeric rank.
    pub fn rank(self) -> u8 {
        match self {
            Self::Minor => 0,
            Self::Low => 1,
            Self::Normal => 2,
            Self::High => 3,
            Self::Major => 4,
        }
    }

    /// Parse one level name in any letter case.
    pub fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "minor" => Some(Self::Minor),
            "low" => Some(Self::Low),
            "normal" => Some(Self::Normal),
            "high" => Some(Self::High),
            "major" => Some(Self::Major),
            _ => None,
        }
    }
}

impl<'de> Deserialize<'de> for Level {
    fn deserialize<Deserializer>(deserializer: Deserializer) -> Result<Self, Deserializer::Error>
    where
        Deserializer: serde::Deserializer<'de>,
    {
        let name = String::deserialize(deserializer)?;
        Level::parse(&name).ok_or_else(|| {
            serde::de::Error::unknown_variant(&name, &["minor", "low", "normal", "high", "major"])
        })
    }
}

/// One patch destination over a single route or a filter.
#[derive(Debug, Clone)]
pub enum PatchTarget {
    /// Single destination route.
    Target(Route),
    /// Callback over routes.
    Filter(Callback<FilterArgs>),
}

impl PatchTarget {
    /// Build one filter destination from its callback.
    pub fn from_filter(filter: Function) -> Self {
        Self::Filter(Callback::new(filter))
    }
}

/// One patch shape.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchShape {
    /// The patch target route.
    #[serde(default)]
    target: Option<Route>,
    /// The structured format under patching with an rc default.
    #[serde(default)]
    format: Option<String>,
    /// The patch merge level. Higher ranks apply first.
    #[serde(default)]
    priority: Level,
}

/// One structured patch declaration.
#[derive(Debug, Clone)]
pub struct StructuredPatch {
    /// The patch destination over a route or a filter.
    pub target: PatchTarget,
    /// The structured format under patching.
    pub format: StructuredFormat,
    /// The patch callback over live data.
    pub apply: Callback<ApplyArgs<StructuredDocument>>,
    /// The patch merge level. Higher ranks apply first.
    pub priority: Level,
    /// The owner config name.
    pub owner: Option<String>,
}

confit_surface_face::table_face!(StructuredPatch, {
    optional target: Named::<Route>::ty(),
    optional filter: Callback::<FilterArgs>::ty(),
    required format: Named::<StructuredFormat>::ty(),
    required apply: Callback::<ApplyArgs<StructuredDocument>>::ty(),
    optional priority: Level::ty(),
});

impl StructuredPatch {
    /// Build one structured patch.
    pub fn new(
        target: PatchTarget,
        format: StructuredFormat,
        apply: Function,
        priority: Level,
        owner: Option<String>,
    ) -> Self {
        Self {
            target,
            format,
            apply: Callback::new(apply),
            priority,
            owner,
        }
    }
}

impl FromLua for StructuredPatch {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = patch_table(&value)?;
        let shape: PatchShape = decode_stripped_shape(lua, &table)?;
        let filter = patch_optional_filter(&table, lua)?;
        let target = decode_destination(shape.target, filter)?;
        let format = patch_format(shape.format.as_deref()).map_err(mlua::Error::from)?;
        let apply = patch_required_apply(lua, &table)?;
        Ok(Self {
            target,
            format,
            apply,
            priority: shape.priority,
            owner: None,
        })
    }
}

/// One rc patch declaration.
#[derive(Debug, Clone)]
pub struct RcPatch {
    /// The patch destination over a route or a filter.
    pub target: PatchTarget,
    /// The patch callback over live sections.
    pub apply: Callback<ApplyArgs<RcDocument>>,
    /// The patch merge level. Higher ranks apply first.
    pub priority: Level,
    /// The owner config name.
    pub owner: Option<String>,
}

confit_surface_face::table_face!(RcPatch, {
    optional target: Named::<Route>::ty(),
    optional filter: Callback::<FilterArgs>::ty(),
    required apply: Callback::<ApplyArgs<RcDocument>>::ty(),
    optional priority: Level::ty(),
});

impl RcPatch {
    /// Build one rc patch.
    pub fn new(
        target: PatchTarget,
        apply: Function,
        priority: Level,
        owner: Option<String>,
    ) -> Self {
        Self {
            target,
            apply: Callback::new(apply),
            priority,
            owner,
        }
    }
}

impl FromLua for RcPatch {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = patch_table(&value)?;
        let shape: PatchShape = decode_stripped_shape(lua, &table)?;
        if shape.format.is_some() {
            return Err(ApiError::mismatch(
                "patch key",
                "one of 'target', 'filter', 'apply' or 'priority'",
                "'format'",
            )
            .into());
        }
        let filter = patch_optional_filter(&table, lua)?;
        let target = decode_destination(shape.target, filter)?;
        let apply = patch_required_apply(lua, &table)?;
        Ok(Self {
            target,
            apply,
            priority: shape.priority,
            owner: None,
        })
    }
}

// The union face sits beside its manual FromLua twin.
confit_surface_face::union_face! {
    /// One patch declaration in either shape.
    #[derive(Debug, Clone)]
    pub enum Patch {
        /// Structured data payload patch.
        Structured(StructuredPatch),
        /// Rc section payload patch.
        Rc(RcPatch),
    }
}

impl FromLua for Patch {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        decode_patch(lua, &value, None).map_err(mlua::Error::from)
    }
}

impl Patch {
    /// Stamp one owner config name over the patch.
    fn set_owner(&mut self, owner: &str) {
        match self {
            Self::Structured(found) => found.owner = Some(owner.to_owned()),
            Self::Rc(found) => found.owner = Some(owner.to_owned()),
        }
    }
}

/// Decode one patch table into its declaration.
///
/// - Config names stamp the patch owner.
/// - Patches take a target or a filter with both failing.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped targets, filters, formats
///   and callbacks.
/// - [`ApiError::Lua`] for Lua read failures.
pub(crate) fn decode_patch(
    lua: &Lua,
    value: &Value,
    owner: Option<&str>,
) -> crate::error::Result<Patch> {
    let table = patch_table(value)?;
    let mut patch = if patch_has_format(&table)? {
        Patch::Structured(StructuredPatch::from_lua(value.clone(), lua).map_err(api_of)?)
    } else {
        Patch::Rc(RcPatch::from_lua(value.clone(), lua).map_err(api_of)?)
    };
    if let Some(name) = owner {
        patch.set_owner(name);
    }
    Ok(patch)
}

/// Decode one patch destination over a route or a filter.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for missing and doubled destinations.
fn decode_destination(
    target: Option<Route>,
    filter: Option<Callback<FilterArgs>>,
) -> mlua::Result<PatchTarget> {
    match (target, filter) {
        (Some(route), None) => Ok(PatchTarget::Target(route)),
        (None, Some(filter)) => Ok(PatchTarget::Filter(filter)),
        (Some(_), Some(_)) => Err(ApiError::mismatch(
            "patch target",
            "either 'target' or 'filter'",
            "'target' with 'filter'",
        )
        .into()),
        (None, None) => {
            Err(ApiError::mismatch("patch target", "either 'target' or 'filter'", "nil").into())
        }
    }
}

/// Decode one patch table from its value.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-table patches.
fn patch_table(value: &Value) -> crate::error::Result<Table> {
    match value {
        Value::Table(table) => Ok(table.clone()),
        _ => Err(ApiError::mismatch(
            "patch",
            "a patch table",
            received(value),
        )),
    }
}

/// Report whether one patch table holds the format marker.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-string keys.
fn patch_has_format(table: &Table) -> crate::error::Result<bool> {
    for pair in table.pairs::<Value, Value>() {
        let (key, _) = pair?;
        match key {
            Value::String(text) => {
                if text.to_string_lossy() == "format" {
                    return Ok(true);
                }
            }
            _ => {
                return Err(ApiError::mismatch(
                    "patch key",
                    "a string key",
                    received(&key),
                ));
            }
        }
    }
    Ok(false)
}

/// Decode one patch shape past its function callbacks.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped targets, formats, priorities and
///   owners.
/// - [`mlua::Error`] for Lua read failures.
fn decode_stripped_shape(lua: &Lua, table: &Table) -> mlua::Result<PatchShape> {
    let probe = lua.create_table()?;
    for pair in table.pairs::<Value, Value>() {
        let (key, found) = pair?;
        let skip = match &key {
            Value::String(text) => {
                let name = text.to_string_lossy();
                name == "apply" || name == "filter"
            }
            _ => false,
        };
        if !skip {
            probe.set(key, found)?;
        }
    }
    decode_named_shape(Value::Table(probe), "patch")
}

/// Decode one optional patch filter callback.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for non-function filters.
fn patch_optional_filter(table: &Table, lua: &Lua) -> mlua::Result<Option<Callback<FilterArgs>>> {
    let item: Value = table.get("filter")?;
    match item {
        Value::Nil => Ok(None),
        _ => Callback::from_lua(item.clone(), lua)
            .map(Some)
            .map_err(|_| ApiError::mismatch("patch filter", "a function", received(&item)).into()),
    }
}

/// Decode one required patch callback over its live document type.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for missing and non-function callbacks.
fn patch_required_apply<Document: Typed>(
    lua: &Lua,
    table: &Table,
) -> mlua::Result<Callback<ApplyArgs<Document>>> {
    let item: Value = table.get("apply")?;
    Callback::from_lua(item.clone(), lua)
        .map_err(|_| ApiError::mismatch("patch apply", "a function", received(&item)).into())
}

/// Decode one patch format name through its structured shape.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for missing and unknown formats.
fn patch_format(format: Option<&str>) -> crate::error::Result<StructuredFormat> {
    const WANT: &str = "one of 'json', 'toml' or 'yaml'";
    match format {
        Some(name) => match StructuredFormat::parse(name) {
            Some(format) => Ok(format),
            None => Err(ApiError::mismatch(
                "patch format",
                WANT,
                format!("'{name}'"),
            )),
        },
        None => Err(ApiError::mismatch("patch format", WANT, "nil")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use confit_model::document::StructuredFormat;

    fn test_lua() -> Lua {
        Lua::new()
    }

    fn config_of(lua: &Lua, source: &str) -> Value {
        lua.load(source).call(()).unwrap()
    }

    #[test]
    fn rc_foreign_shapes_fail_naming_keys() {
        let lua = test_lua();
        let cases = [
            (
                r#"return { target = { base = "home", relative = "a" }, apply = function(d) return d end, nope = 1 }"#,
                "nope",
            ),
            (
                r#"return { target = 1, apply = function(d) return d end }"#,
                "a route table",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, apply = 1 }"#,
                "a function",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, format = "json", nope = 1 }"#,
                "nope",
            ),
        ];
        for (source, want) in cases {
            let value = config_of(&lua, source);
            match decode_patch(&lua, &value, None) {
                Ok(_) => panic!("foreign rc patch passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(want), "the text shows the fault: {text}");
                }
            }
        }
    }

    #[test]
    fn patch_decodes_target_with_format_and_callback() {
        let lua = test_lua();
        let value = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.json" }, format = "toml", apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &value, None) {
            Ok(Patch::Structured(found)) => {
                match &found.target {
                    PatchTarget::Target(route) => {
                        assert_eq!(route.display(), "home('app.json')");
                    }
                    PatchTarget::Filter(_) => panic!("target means route"),
                }
                assert_eq!(found.format, StructuredFormat::Toml);
            }
            Ok(Patch::Rc(_)) => panic!("format presence means structured"),
            Err(error) => panic!("patch decodes: {error}"),
        }
        let bare = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.sh" }, apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &bare, None) {
            Ok(Patch::Rc(found)) => match &found.target {
                PatchTarget::Target(route) => {
                    assert_eq!(route.display(), "home('app.sh')");
                }
                PatchTarget::Filter(_) => panic!("target means route"),
            },
            Ok(Patch::Structured(_)) => panic!("format absence means rc"),
            Err(error) => panic!("rc patch decodes: {error}"),
        }
    }

    #[test]
    fn foreign_patch_shapes_fail() {
        let lua = test_lua();
        let cases = [
            (
                r#"return { target = 1, format = "json", apply = function(d) return d end }"#,
                "a route table",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, format = "xml", apply = function(d) return d end }"#,
                "'xml'",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, format = "json", apply = 1 }"#,
                "a function",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, format = "json", apply = function(d) return d end, nope = 1 }"#,
                "nope",
            ),
        ];
        for (source, want) in cases {
            let value = config_of(&lua, source);
            match decode_patch(&lua, &value, None) {
                Ok(_) => panic!("foreign patch passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(want), "the text shows the fault: {text}");
                }
            }
        }
    }

    #[test]
    fn levels_parse_all_names_in_any_case() {
        for (name, level) in [
            ("minor", Level::Minor),
            ("MINOR", Level::Minor),
            ("MiNoR", Level::Minor),
            ("low", Level::Low),
            ("LOW", Level::Low),
            ("normal", Level::Normal),
            ("NORMAL", Level::Normal),
            ("high", Level::High),
            ("HIGH", Level::High),
            ("major", Level::Major),
            ("MAJOR", Level::Major),
        ] {
            assert_eq!(Level::parse(name), Some(level), "parses any case: {name}");
        }
        assert_eq!(Level::parse("urgent"), None);
        assert_eq!(Level::default(), Level::Normal);
        assert_eq!(
            [
                Level::Minor.rank(),
                Level::Low.rank(),
                Level::Normal.rank(),
                Level::High.rank(),
                Level::Major.rank(),
            ],
            [0, 1, 2, 3, 4],
        );
    }

    #[test]
    fn absent_priorities_read_normal() {
        let lua = test_lua();
        let structured = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.json" }, format = "json", apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &structured, None) {
            Ok(Patch::Structured(found)) => assert_eq!(found.priority, Level::Normal),
            Ok(Patch::Rc(found)) => assert_eq!(found.priority, Level::Normal),
            Err(error) => panic!("absent priority decodes: {error}"),
        }
        let bare = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.sh" }, apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &bare, None) {
            Ok(Patch::Structured(found)) => assert_eq!(found.priority, Level::Normal),
            Ok(Patch::Rc(found)) => assert_eq!(found.priority, Level::Normal),
            Err(error) => panic!("absent rc priority decodes: {error}"),
        }
        let ranked = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.json" }, format = "json", apply = function(d) return d end, priority = "HIGH" }"#,
        );
        match decode_patch(&lua, &ranked, None) {
            Ok(Patch::Structured(found)) => assert_eq!(found.priority, Level::High),
            Ok(Patch::Rc(found)) => assert_eq!(found.priority, Level::High),
            Err(error) => panic!("ranked priority decodes: {error}"),
        }
    }

    #[test]
    fn foreign_priorities_fail_naming_field() {
        let lua = test_lua();
        let cases = [
            (
                r#"return { target = { base = "home", relative = "a" }, format = "json", apply = function(d) return d end, priority = "urgent" }"#,
                "patch priority",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, format = "json", apply = function(d) return d end, priority = 1.5 }"#,
                "patch priority",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, apply = function(d) return d end, priority = true }"#,
                "patch priority",
            ),
            (
                r#"return { target = { base = "home", relative = "a" }, apply = function(d) return d end, owner = "table" }"#,
                "unknown field",
            ),
        ];
        for (source, want) in cases {
            let value = config_of(&lua, source);
            match decode_patch(&lua, &value, None) {
                Ok(_) => panic!("foreign priority passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(text.contains(want), "the text shows the field: {text}");
                }
            }
        }
    }

    #[test]
    fn table_owners_fail_naming_keys() {
        let lua = test_lua();
        let value = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "a" }, apply = function(d) return d end, owner = "table" }"#,
        );
        match decode_patch(&lua, &value, None) {
            Ok(_) => panic!("table owner passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("owner"), "the text shows the key: {text}");
            }
        }
    }

    #[test]
    fn config_names_stamp_owners() {
        let lua = test_lua();
        let value = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "a" }, apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &value, None) {
            Ok(Patch::Structured(found)) => assert_eq!(found.owner.as_deref(), None),
            Ok(Patch::Rc(found)) => assert_eq!(found.owner.as_deref(), None),
            Err(error) => panic!("patch decodes: {error}"),
        }
        match decode_patch(&lua, &value, Some("config")) {
            Ok(Patch::Structured(found)) => assert_eq!(found.owner.as_deref(), Some("config")),
            Ok(Patch::Rc(found)) => assert_eq!(found.owner.as_deref(), Some("config")),
            Err(error) => panic!("config owner stamps: {error}"),
        }
    }

    #[test]
    fn filters_decode_callbacks() {
        let lua = test_lua();
        let structured = config_of(
            &lua,
            r#"return { filter = function(route) return true end, format = "json", apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &structured, None) {
            Ok(Patch::Structured(found)) => match &found.target {
                PatchTarget::Filter(filter) => {
                    let _: &Function = filter.as_function();
                }
                PatchTarget::Target(_) => panic!("filter means many"),
            },
            Ok(Patch::Rc(_)) => panic!("format presence means structured"),
            Err(error) => panic!("filter decodes: {error}"),
        }
        let bare = config_of(
            &lua,
            r#"return { filter = function(route) return true end, apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &bare, None) {
            Ok(Patch::Rc(found)) => match &found.target {
                PatchTarget::Filter(filter) => {
                    let _: &Function = filter.as_function();
                }
                PatchTarget::Target(_) => panic!("filter means many"),
            },
            Ok(Patch::Structured(_)) => panic!("format absence means rc"),
            Err(error) => panic!("rc filter decodes: {error}"),
        }
    }

    #[test]
    fn foreign_filters_fail() {
        let lua = test_lua();
        let value = config_of(
            &lua,
            r#"return { filter = { kind = "structured" }, format = "json", apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &value, None) {
            Ok(_) => panic!("table filter passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(
                    text.contains("a function"),
                    "the text shows the want: {text}"
                );
            }
        }
    }

    #[test]
    fn doubled_destinations_fail() {
        let lua = test_lua();
        let cases = [
            r#"return { target = { base = "home", relative = "a" }, filter = function(route) return true end, format = "json", apply = function(d) return d end }"#,
            r#"return { target = { base = "home", relative = "a" }, filter = function(route) return true end, apply = function(d) return d end }"#,
        ];
        for source in cases {
            let value = config_of(&lua, source);
            match decode_patch(&lua, &value, None) {
                Ok(_) => panic!("doubled destination passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(
                        text.contains("either 'target' or 'filter'"),
                        "the text shows the want: {text}"
                    );
                    assert!(
                        text.contains("'target' with 'filter'"),
                        "the text shows the fault: {text}"
                    );
                }
            }
        }
    }

    #[test]
    fn missing_destinations_fail() {
        let lua = test_lua();
        let cases = [
            r#"return { format = "json", apply = function(d) return d end }"#,
            r#"return { apply = function(d) return d end }"#,
        ];
        for source in cases {
            let value = config_of(&lua, source);
            match decode_patch(&lua, &value, None) {
                Ok(_) => panic!("missing destination passes: {source}"),
                Err(error) => {
                    let text = error.to_string();
                    assert!(
                        text.contains("either 'target' or 'filter'"),
                        "the text shows the want: {text}"
                    );
                }
            }
        }
    }
}
