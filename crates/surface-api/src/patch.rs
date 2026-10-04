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
    /// Reads the numeric rank.
    pub fn rank(self) -> u8 {
        match self {
            Self::Minor => 0,
            Self::Low => 1,
            Self::Normal => 2,
            Self::High => 3,
            Self::Major => 4,
        }
    }

    /// Parses one level name in any letter case.
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

/// One patch shape.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PatchShape {
    /// The patch target route.
    target: Route,
    /// The structured format under patching. None rides rc.
    #[serde(default)]
    format: Option<String>,
    /// The patch merge level. Higher ranks apply first.
    #[serde(default)]
    priority: Level,
}

/// One structured patch declaration.
#[derive(Debug, Clone)]
pub struct StructuredPatch {
    /// The patch target route.
    pub target: Route,
    /// The structured format under patching.
    pub format: StructuredFormat,
    /// The patch callback over live data.
    pub apply: Callback<StructuredDocument>,
    /// The patch merge level. Higher ranks apply first.
    pub priority: Level,
    /// The owner config name.
    pub owner: Option<String>,
}

confit_surface_face::table_face!(StructuredPatch, {
    required target: Named::<Route>::ty(),
    required format: Named::<StructuredFormat>::ty(),
    required apply: Callback::<StructuredDocument>::ty(),
    optional priority: Level::ty(),
});

impl StructuredPatch {
    /// Builds one structured patch.
    pub fn new(
        target: Route,
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

    /// Reads the patch target route.
    pub fn target(&self) -> &Route {
        &self.target
    }

    /// Reads the structured format under patching.
    pub fn format(&self) -> StructuredFormat {
        self.format
    }

    /// Reads the patch callback over live data.
    pub fn apply(&self) -> &Function {
        self.apply.as_function()
    }

    /// Reads the patch merge level.
    pub fn priority(&self) -> Level {
        self.priority
    }

    /// Reads the owner config name.
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }
}

impl FromLua for StructuredPatch {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = patch_table(&value)?;
        let shape: PatchShape = decode_stripped_shape(lua, &table)?;
        let format = patch_format(shape.format.as_deref()).map_err(mlua::Error::from)?;
        let apply = patch_required_apply(lua, &table)?;
        Ok(Self {
            target: shape.target,
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
    /// The patch target route.
    pub target: Route,
    /// The patch callback over live sections.
    pub apply: Callback<RcDocument>,
    /// The patch merge level. Higher ranks apply first.
    pub priority: Level,
    /// The owner config name.
    pub owner: Option<String>,
}

confit_surface_face::table_face!(RcPatch, {
    required target: Named::<Route>::ty(),
    required apply: Callback::<RcDocument>::ty(),
    optional priority: Level::ty(),
});

impl RcPatch {
    /// Builds one rc patch.
    pub fn new(target: Route, apply: Function, priority: Level, owner: Option<String>) -> Self {
        Self {
            target,
            apply: Callback::new(apply),
            priority,
            owner,
        }
    }

    /// Reads the patch target route.
    pub fn target(&self) -> &Route {
        &self.target
    }

    /// Reads the patch callback over live sections.
    pub fn apply(&self) -> &Function {
        self.apply.as_function()
    }

    /// Reads the patch merge level.
    pub fn priority(&self) -> Level {
        self.priority
    }

    /// Reads the owner config name.
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }
}

impl FromLua for RcPatch {
    fn from_lua(value: Value, lua: &Lua) -> mlua::Result<Self> {
        let table = patch_table(&value)?;
        let shape: PatchShape = decode_stripped_shape(lua, &table)?;
        if shape.format.is_some() {
            return Err(ApiError::mismatch(
                "patch key",
                "one of 'target', 'apply' or 'priority'",
                "'format'",
            )
            .into());
        }
        let apply = patch_required_apply(lua, &table)?;
        Ok(Self {
            target: shape.target,
            apply,
            priority: shape.priority,
            owner: None,
        })
    }
}

// The union face rides beside its manual FromLua twin.
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
    /// Reads the patch target route.
    pub fn target(&self) -> &Route {
        match self {
            Self::Structured(found) => found.target(),
            Self::Rc(found) => found.target(),
        }
    }

    /// Reads the patch callback over live data.
    pub fn apply(&self) -> &Function {
        match self {
            Self::Structured(found) => found.apply(),
            Self::Rc(found) => found.apply(),
        }
    }

    /// Reads the patch merge level.
    pub fn priority(&self) -> Level {
        match self {
            Self::Structured(found) => found.priority(),
            Self::Rc(found) => found.priority(),
        }
    }

    /// Reads the owner config name.
    pub fn owner(&self) -> Option<&str> {
        match self {
            Self::Structured(found) => found.owner(),
            Self::Rc(found) => found.owner(),
        }
    }

    /// Stamps one owner config name over the patch.
    fn set_owner(&mut self, owner: &str) {
        match self {
            Self::Structured(found) => found.owner = Some(owner.to_owned()),
            Self::Rc(found) => found.owner = Some(owner.to_owned()),
        }
    }
}

/// Decodes one patch table into its declaration.
///
/// - The config name stamps the patch owner.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for misshaped targets, formats and callbacks.
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

/// Decodes one patch table from its value.
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

/// Reports whether one patch table holds the format marker.
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

/// Decodes one patch shape past its function callback.
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
            Value::String(text) => text.to_string_lossy() == "apply",
            _ => false,
        };
        if !skip {
            probe.set(key, found)?;
        }
    }
    decode_named_shape(Value::Table(probe), "patch")
}

/// Decodes one required patch callback over its live document type.
///
/// # Errors
///
/// - [`ApiError::Mismatch`] for missing and non-function callbacks.
fn patch_required_apply<Document: Typed>(
    lua: &Lua,
    table: &Table,
) -> mlua::Result<Callback<Document>> {
    let item: Value = table.get("apply")?;
    Callback::from_lua(item.clone(), lua)
        .map_err(|_| ApiError::mismatch("patch apply", "a function", received(&item)).into())
}

/// Decodes one patch format name through its structured shape.
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
                    assert!(text.contains(want), "names the fault: {text}");
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
                assert_eq!(found.target().display(), "home('app.json')");
                assert_eq!(found.format(), StructuredFormat::Toml);
            }
            Ok(Patch::Rc(_)) => panic!("format presence rides structured"),
            Err(error) => panic!("patch decodes: {error}"),
        }
        let bare = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.sh" }, apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &bare, None) {
            Ok(Patch::Rc(found)) => {
                assert_eq!(found.target().display(), "home('app.sh')");
            }
            Ok(Patch::Structured(_)) => panic!("format absence rides rc"),
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
                    assert!(text.contains(want), "names the fault: {text}");
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
            Ok(patch) => assert_eq!(patch.priority(), Level::Normal),
            Err(error) => panic!("absent priority decodes: {error}"),
        }
        let bare = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.sh" }, apply = function(d) return d end }"#,
        );
        match decode_patch(&lua, &bare, None) {
            Ok(patch) => assert_eq!(patch.priority(), Level::Normal),
            Err(error) => panic!("absent rc priority decodes: {error}"),
        }
        let ranked = config_of(
            &lua,
            r#"return { target = { base = "home", relative = "app.json" }, format = "json", apply = function(d) return d end, priority = "HIGH" }"#,
        );
        match decode_patch(&lua, &ranked, None) {
            Ok(patch) => assert_eq!(patch.priority(), Level::High),
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
                    assert!(text.contains(want), "names the field: {text}");
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
                assert!(text.contains("owner"), "names the key: {text}");
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
            Ok(patch) => assert_eq!(patch.owner(), None),
            Err(error) => panic!("patch decodes: {error}"),
        }
        match decode_patch(&lua, &value, Some("config")) {
            Ok(patch) => assert_eq!(patch.owner(), Some("config")),
            Err(error) => panic!("config owner stamps: {error}"),
        }
    }
}
