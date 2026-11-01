//! Declares patches over routes with callbacks.

use mlua::{Lua, Table, Value};

use lua_binding::Shape;
use lua_binding::callable::Callable;
use lua_binding::error::{DecodeError, DecodeTag};
use lua_binding::value::Decode;
use serde::{Deserialize, Serialize};

use confit_model::document::{RcData, StructuredDocument, StructuredFormat};
use confit_model::hook::Hook;
use confit_model::routes::Route;

#[derive(Debug, Clone, PartialEq, Eq, Shape)]
#[shape(table, name = "Document_Structured")]
pub struct StructuredLiveDocument {
    /// The destination route.
    pub path: Route,
    /// The structured body under patching.
    #[shape(flatten)]
    pub body: StructuredDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Shape)]
#[shape(table, name = "Document_Rc")]
pub struct RcLiveDocument {
    /// The destination route.
    pub path: Route,
    /// The rc body under patching.
    #[shape(flatten)]
    pub body: RcData,
}

#[allow(dead_code)]
#[derive(Shape)]
#[shape(function)]
pub struct StructuredApplyArgs {
    /// The live structured document.
    #[shape(borrowed)]
    document: StructuredLiveDocument,
}

#[allow(dead_code)]
#[derive(Shape)]
#[shape(function)]
pub struct RcApplyArgs {
    /// The live rc document.
    #[shape(borrowed)]
    document: RcLiveDocument,
}

#[allow(dead_code)]
#[derive(Shape)]
#[shape(function)]
pub struct HookArgs {
    /// The live hook.
    #[shape(borrowed)]
    hook: Hook,
}

#[allow(dead_code)]
#[derive(Shape)]
#[shape(function, returns = bool)]
pub struct FilterArgs {
    /// The target route of the candidate document.
    route: Route,
}

/// The patch merge level.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Shape)]
#[serde(rename_all = "lowercase")]
#[shape(table, str_alias)]
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

impl Level {
    /// Returns the numeric rank.
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

/// One route or a filter.
#[derive(Debug, Clone)]
pub enum PatchTarget {
    /// Single destination route.
    Target(Route),
    /// Callback over routes.
    Filter(Callable<FilterArgs>),
}

/// One structured patch table.
#[derive(Debug, Clone, Shape)]
#[shape(table, validate = reject_structured_destination)]
pub struct StructuredPatch {
    /// The patch target route.
    pub target: Option<Route>,
    /// The patch filter callback.
    pub filter: Option<Callable<FilterArgs>>,
    /// The structured format under patching.
    pub format: StructuredFormat,
    /// The patch callback over the live document.
    pub apply: Callable<StructuredApplyArgs>,
    /// The patch merge level. Higher levels apply first.
    #[shape(default)]
    pub priority: Level,
}

/// One rc patch table.
#[derive(Debug, Clone, Shape)]
#[shape(table, validate = reject_rc_destination)]
pub struct RcPatch {
    /// The patch target route.
    pub target: Option<Route>,
    /// The patch filter callback.
    pub filter: Option<Callable<FilterArgs>>,
    /// The patch callback over the live document.
    pub apply: Callable<RcApplyArgs>,
    /// The patch merge level. Higher levels apply first.
    #[shape(default)]
    pub priority: Level,
}

/// One hook patch table.
#[derive(Debug, Clone, Shape)]
#[shape(table)]
pub struct HookPatch {
    /// The hook id under patching.
    pub target: String,
    /// The hook callback over the live hook.
    pub apply: Callable<HookArgs>,
}

/// One patch table in either shape.
#[derive(Debug, Clone, Shape)]
#[shape(table, untagged, probe = probe_patch)]
pub enum Patch {
    /// Structured data payload patch.
    Structured(StructuredPatch),
    /// Rc section payload patch.
    Rc(RcPatch),
    /// Hook payload patch.
    Hook(HookPatch),
}

/// Picks the patch shape.
///
/// # Errors
///
/// - [`DecodeError`] when the table is unreadable.
fn probe_patch(lua: &Lua, table: &Table) -> std::result::Result<Patch, DecodeError> {
    let target = table.get::<Value>("target").map_err(|error| DecodeError {
        segments: Vec::new(),
        reason: DecodeTag::ShapeMismatch {
            require: "readable entries".to_owned(),
            found: error.to_string(),
        },
    })?;
    if matches!(target, Value::String(_)) {
        return HookPatch::decode(&Value::Table(table.clone()), lua).map(Patch::Hook);
    }
    let raw = table.get::<Value>("format").map_err(|error| DecodeError {
        segments: Vec::new(),
        reason: DecodeTag::ShapeMismatch {
            require: "readable entries".to_owned(),
            found: error.to_string(),
        },
    })?;
    if matches!(raw, Value::Nil) {
        RcPatch::decode(&Value::Table(table.clone()), lua).map(Patch::Rc)
    } else {
        StructuredPatch::decode(&Value::Table(table.clone()), lua).map(Patch::Structured)
    }
}

/// Resolves the patch destination.
///
/// # Errors
///
/// - [`DecodeError`] when the destination is missing or doubled.
pub fn join_destination(
    target: Option<Route>,
    filter: Option<Callable<FilterArgs>>,
) -> std::result::Result<PatchTarget, DecodeError> {
    match (target, filter) {
        (Some(route), None) => Ok(PatchTarget::Target(route)),
        (None, Some(filter)) => Ok(PatchTarget::Filter(filter)),
        (Some(_), Some(_)) => Err(DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "either 'target' or 'filter'".to_owned(),
                found: "'target' with 'filter'".to_owned(),
            },
        }),
        (None, None) => Err(DecodeError {
            segments: Vec::new(),
            reason: DecodeTag::ShapeMismatch {
                require: "either 'target' or 'filter'".to_owned(),
                found: "nil".to_owned(),
            },
        }),
    }
}

/// Rejects a structured shape with a missing or doubled destination.
///
/// # Errors
///
/// - [`mlua::Error`] when the destination is missing or doubled.
fn reject_structured_destination(patch: &StructuredPatch) -> mlua::Result<()> {
    join_destination(patch.target.clone(), patch.filter.clone())
        .map_err(|mismatch| mlua::Error::external(mismatch.to_string()))?;
    Ok(())
}

/// Rejects an rc shape with a missing or doubled destination.
///
/// # Errors
///
/// - [`mlua::Error`] when the destination is missing or doubled.
fn reject_rc_destination(patch: &RcPatch) -> mlua::Result<()> {
    join_destination(patch.target.clone(), patch.filter.clone())
        .map_err(|mismatch| mlua::Error::external(mismatch.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lua_binding::generator::Generator;
    use lua_binding::shape::Shape;

    #[test]
    fn filter_stubs_show_boolean_returns_while_apply_stays_bare() {
        let mut collector = Generator::new();
        StructuredPatch::collect(&mut collector);
        RcPatch::collect(&mut collector);
        HookPatch::collect(&mut collector);
        let text = collector.generate();
        assert!(
            text.contains("fun(route: Route): boolean"),
            "the filter stub shows a boolean: {text}"
        );
        for holder in ["StructuredPatch", "RcPatch"] {
            assert!(
                text.contains("@field filter (fun(route: Route): boolean)?"),
                "the filter field shows a boolean: {text}"
            );
            let method = format!("function {holder}.filter(route) end");
            assert!(
                text.contains(&method),
                "the filter method is present: {text}"
            );
        }
        for block in text.split("\n\n") {
            if block.contains(".filter(route)") {
                assert!(
                    block.contains("@return boolean"),
                    "the filter returns a boolean: {block}"
                );
            }
            if block.contains(".apply(") {
                assert!(
                    !block.contains("@return"),
                    "the apply callback is not wrapped: {block}"
                );
            }
        }
        assert!(
            text.contains("@field apply fun(document: Document_Structured)"),
            "the structured apply callback is not wrapped: {text}"
        );
        assert!(
            text.contains("@field apply fun(document: Document_Rc)"),
            "the rc apply callback is not wrapped: {text}"
        );
        assert!(
            text.contains("@field apply fun(hook: Hook)"),
            "the hook apply callback is not wrapped: {text}"
        );
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
            assert_eq!(
                Level::parse(name),
                Some(level),
                "the level parser accepts any case: {name}"
            );
        }
        assert_eq!(Level::parse("urgent"), None);
    }
}
