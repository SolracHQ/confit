//! Patches
//! Patch declarations over routes with callbacks.

use mlua::{Function, Lua, Table, Value};

use confit_binding::callback::Callback;
use confit_binding::value::{Decode, Mismatch};
use confit_binding_macros::Face;
use serde::{Deserialize, Serialize};

use confit_model::document::StructuredFormat;
use confit_model::hook::Hook;
use confit_model::routes::Route;

/// Face for patch apply callbacks.
#[allow(dead_code)]
#[derive(Face)]
#[face(callback_shape)]
pub struct ApplyArgs {
    /// Target route of the patched document.
    route: Table,
    /// Current document under patching.
    #[face(live)]
    document: Table,
}

/// Face for hook patch callbacks.
#[allow(dead_code)]
#[derive(Face)]
#[face(callback_shape)]
pub struct HookArgs {
    /// Live hook under patching.
    #[face(live)]
    hook: Hook,
}

/// Face for patch filter callbacks.
#[allow(dead_code)]
#[derive(Face)]
#[face(callback_shape)]
pub struct FilterArgs {
    /// Target route of the candidate document.
    route: Table,
}

/// Patch merge level.
///
/// - Names parse case-insensitively.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Face)]
#[serde(rename_all = "lowercase")]
#[face(table, string)]
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

/// One structured patch table.
#[derive(Debug, Clone, Face)]
#[face(table, validate = reject_structured_destination)]
pub struct StructuredPatch {
    /// The patch target route.
    pub target: Option<Route>,
    /// The patch filter callback.
    pub filter: Option<Function>,
    /// The structured format under patching.
    pub format: StructuredFormat,
    /// The patch callback over current data.
    pub apply: Function,
    /// The patch merge level. Higher levels apply first.
    #[face(default)]
    pub priority: Level,
}

/// One rc patch table.
#[derive(Debug, Clone, Face)]
#[face(table, validate = reject_rc_destination)]
pub struct RcPatch {
    /// The patch target route.
    pub target: Option<Route>,
    /// The patch filter callback.
    pub filter: Option<Function>,
    /// The patch callback over current sections.
    pub apply: Function,
    /// The patch merge level. Higher levels apply first.
    #[face(default)]
    pub priority: Level,
}

/// One hook patch table.
#[derive(Debug, Clone, Face)]
#[face(table)]
pub struct HookPatch {
    /// The hook id under patching.
    pub target: String,
    /// The hook callback over the live hook.
    pub apply: Callback<HookArgs>,
}

/// One patch table in either shape.
#[derive(Debug, Clone, Face)]
#[face(table, untagged, probe = probe_patch)]
pub enum Patch {
    /// Structured data payload patch.
    Structured(StructuredPatch),
    /// Rc section payload patch.
    Rc(RcPatch),
    /// Hook payload patch.
    Hook(HookPatch),
}

/// Route one patch table into its face over the target shape.
///
/// String targets route hook patches, route targets route document patches
/// over the format marker.
///
/// # Errors
///
/// - [`Mismatch`] for unreadable tables.
fn probe_patch(lua: &Lua, table: &Table) -> std::result::Result<Patch, Mismatch> {
    let target = table.get::<Value>("target").map_err(|error| Mismatch {
        path: "Patch".to_owned(),
        want: "readable entries".to_owned(),
        found: error.to_string(),
    })?;
    if matches!(target, Value::String(_)) {
        return HookPatch::decode(&Value::Table(table.clone()), lua).map(Patch::Hook);
    }
    let raw = table.get::<Value>("format").map_err(|error| Mismatch {
        path: "Patch".to_owned(),
        want: "readable entries".to_owned(),
        found: error.to_string(),
    })?;
    if matches!(raw, Value::Nil) {
        RcPatch::decode(&Value::Table(table.clone()), lua).map(Patch::Rc)
    } else {
        StructuredPatch::decode(&Value::Table(table.clone()), lua).map(Patch::Structured)
    }
}

/// Decode one patch destination over a route or a filter.
///
/// # Errors
///
/// - [`Mismatch`] for missing and doubled destinations.
pub fn join_destination(
    target: Option<Route>,
    filter: Option<Function>,
) -> std::result::Result<PatchTarget, Mismatch> {
    match (target, filter) {
        (Some(route), None) => Ok(PatchTarget::Target(route)),
        (None, Some(filter)) => Ok(PatchTarget::Filter(Callback::new(filter))),
        (Some(_), Some(_)) => Err(Mismatch {
            path: "patch target".to_owned(),
            want: "either 'target' or 'filter'".to_owned(),
            found: "'target' with 'filter'".to_owned(),
        }),
        (None, None) => Err(Mismatch {
            path: "patch target".to_owned(),
            want: "either 'target' or 'filter'".to_owned(),
            found: "nil".to_owned(),
        }),
    }
}

/// Reject structured faces missing or doubling destinations.
///
/// # Errors
///
/// - [`mlua::Error`] for missing and doubled destinations.
fn reject_structured_destination(patch: &StructuredPatch) -> mlua::Result<()> {
    join_destination(patch.target.clone(), patch.filter.clone())
        .map(|_| ())
        .map_err(|mismatch| mlua::Error::external(mismatch.to_string()))
}

/// Reject rc faces missing or doubling destinations.
///
/// # Errors
///
/// - [`mlua::Error`] for missing and doubled destinations.
fn reject_rc_destination(patch: &RcPatch) -> mlua::Result<()> {
    join_destination(patch.target.clone(), patch.filter.clone())
        .map(|_| ())
        .map_err(|mismatch| mlua::Error::external(mismatch.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }
}
