//! Split patch lists.
//! Group structured with rc patches over matched destinations.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use mlua::{Lua, Value};

use crate::api::patch::{FilterArgs, Patch, PatchTarget, StructuredPatch, join_destination};
use crate::api::profile::Profile;
use confit_binding::callback::Callback;
use confit_binding::value::Encode;
use confit_model::document::{Data, RcData, StructuredFormat};
use confit_model::routes::Route;

use super::owned::{OwnedPatch, assemble_config};
use crate::error::{EngineError, Result};

/// Split one profile's patches into owned patches in return order.
pub(super) fn split_patches(profile: &Profile) -> Vec<OwnedPatch> {
    let mut owned = Vec::new();
    for config in &profile.configs {
        owned.extend(assemble_config(config));
    }
    owned
}

/// Group structured patches over matched destinations in rank order.
///
/// - Target patches group under their route.
/// - Filter patches fan out over structured matches.
/// - Fan-out follows destination order.
/// - Vec order feeds the groups, and the stable sort
///   keeps it for equal levels.
///
/// # Errors
///
/// - [`crate::error::EngineError::Mismatch`] for doubled and missing destinations.
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub(super) fn grouped_structured<'a>(
    lua: &Lua,
    placed: &BTreeMap<Route, Data>,
    patches: &'a [OwnedPatch],
) -> Result<BTreeMap<Route, Vec<&'a OwnedPatch>>> {
    let mut grouped: BTreeMap<Route, Vec<&OwnedPatch>> = BTreeMap::new();
    for patch in patches {
        let Some(found) = patch.structured() else {
            continue;
        };
        let target = join_destination(found.target.clone(), found.filter.clone())?;
        for destination in expanded_destinations(lua, placed, &target, is_structured)? {
            grouped.entry(destination).or_default().push(patch);
        }
    }
    for group in grouped.values_mut() {
        group.sort_by_key(|patch| Reverse(rank_of(patch)));
    }
    Ok(grouped)
}

/// Group rc patches over matched destinations in rank order.
///
/// - Target patches group under their route.
/// - Filter patches fan out over rc matches.
/// - Fan-out follows destination order.
/// - Vec order feeds the groups, and the stable sort
///   keeps it for equal levels.
///
/// # Errors
///
/// - [`crate::error::EngineError::Mismatch`] for doubled and missing destinations.
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub(super) fn grouped_rc<'a>(
    lua: &Lua,
    placed: &BTreeMap<Route, Data>,
    patches: &'a [OwnedPatch],
) -> Result<BTreeMap<Route, Vec<&'a OwnedPatch>>> {
    let mut grouped: BTreeMap<Route, Vec<&OwnedPatch>> = BTreeMap::new();
    for patch in patches {
        let Some(found) = patch.rc() else {
            continue;
        };
        let target = join_destination(found.target.clone(), found.filter.clone())?;
        for destination in expanded_destinations(lua, placed, &target, is_rc)? {
            grouped.entry(destination).or_default().push(patch);
        }
    }
    for group in grouped.values_mut() {
        group.sort_by_key(|patch| Reverse(rank_of(patch)));
    }
    Ok(grouped)
}

/// Return the merge rank of one owned patch.
fn rank_of(patch: &OwnedPatch) -> u8 {
    match &patch.patch {
        Patch::Structured(found) => found.priority.rank(),
        Patch::Rc(found) => found.priority.rank(),
        Patch::Hook(_) => 0,
    }
}

/// Collect patch destinations over its target.
///
/// # Errors
///
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
fn expanded_destinations(
    lua: &Lua,
    placed: &BTreeMap<Route, Data>,
    target: &PatchTarget,
    area_matches: fn(&Data) -> bool,
) -> Result<Vec<Route>> {
    match target {
        PatchTarget::Target(route) => Ok(vec![route.clone()]),
        PatchTarget::Filter(filter) => matched_destinations(lua, placed, filter, area_matches),
    }
}

/// Collect filter destinations over callback matches.
///
/// - Matches hold callback results in destination order.
///
/// # Errors
///
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
fn matched_destinations(
    lua: &Lua,
    placed: &BTreeMap<Route, Data>,
    filter: &Callback<FilterArgs>,
    area_matches: fn(&Data) -> bool,
) -> Result<Vec<Route>> {
    let mut matched = Vec::new();
    for (destination, body) in placed {
        if !area_matches(body) {
            continue;
        }
        let Value::Table(route) = destination.encode(lua)? else {
            return Err(EngineError::Lua(mlua::Error::runtime(
                "route views hold tables",
            )));
        };
        let keep = filter.call::<bool>(lua, (&route,))?;
        if keep {
            matched.push(destination.clone());
        }
    }
    Ok(matched)
}

/// Report whether one document holds structured data.
fn is_structured(body: &Data) -> bool {
    matches!(body, Data::Structured { .. })
}

/// Report whether one document holds rc sections.
fn is_rc(body: &Data) -> bool {
    matches!(body, Data::Rc(_))
}

/// Build one structured patch base with its live data.
///
/// # Errors
///
/// - [`EngineError::UnknownTarget`] for missing targets.
/// - [`EngineError::Structured`] for foreign bases with gaps.
pub(super) fn structured_base_of(
    placed: &BTreeMap<Route, Data>,
    target: &Route,
    first: &StructuredPatch,
    patches: &[&StructuredPatch],
) -> Result<(StructuredFormat, BTreeMap<String, serde_json::Value>)> {
    match placed.get(target) {
        Some(body) => {
            let display = target.display();
            structured_declared_base(body, &display, first, patches)
        }
        None => Err(EngineError::UnknownTarget {
            display: target.display(),
        }),
    }
}

/// Build one declared structured patch base with its live data.
///
/// # Errors
///
/// - [`EngineError::Structured`] for foreign bases with gaps.
fn structured_declared_base(
    body: &Data,
    display: &str,
    first: &StructuredPatch,
    patches: &[&StructuredPatch],
) -> Result<(StructuredFormat, BTreeMap<String, serde_json::Value>)> {
    match body {
        Data::Structured { format, data } => {
            for patch in patches {
                if patch.format != *format {
                    return Err(gap(display, format.name(), patch.format.name()));
                }
            }
            let mut live: BTreeMap<String, serde_json::Value> = BTreeMap::new();
            if let Some(map) = data.as_object() {
                for (key, value) in map {
                    live.insert(key.clone(), value.clone());
                }
            }
            Ok((*format, live))
        }
        Data::Rc(_) => Err(gap(display, "rc", first.format.name())),
        _ => Err(gap(display, body.kind().name(), first.format.name())),
    }
}

/// Build one rc patch base with its live sections.
///
/// # Errors
///
/// - [`EngineError::UnknownTarget`] for missing targets.
/// - [`EngineError::Structured`] for foreign bases with gaps.
pub(super) fn rc_base_of(placed: &BTreeMap<Route, Data>, target: &Route) -> Result<RcData> {
    match placed.get(target) {
        Some(body) => match body {
            Data::Rc(found) => Ok(found.clone()),
            Data::Structured { format, .. } => Err(gap(&target.display(), format.name(), "rc")),
            _ => Err(gap(&target.display(), body.kind().name(), "rc")),
        },
        None => Err(EngineError::UnknownTarget {
            display: target.display(),
        }),
    }
}

/// Build one patch gap error over its base with patch names.
fn gap(display: &str, base: &str, patch: &str) -> EngineError {
    EngineError::Structured {
        display: display.to_owned(),
        base: base.to_owned(),
        patch: patch.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::config::Config;
    use crate::api::patch::{Level, Patch, RcPatch, StructuredPatch};
    use confit_model::routes::RouteBase;
    use mlua::{Function, Lua};
    use std::collections::BTreeMap;

    fn profile_of(documents: Vec<(Route, Data)>, patches: Vec<Patch>) -> Profile {
        Profile::new(
            vec![Config::new(
                "tools/test".to_owned(),
                documents.into_iter().collect(),
                patches,
                Vec::new(),
                std::collections::HashMap::new(),
            )],
            std::collections::HashMap::new(),
        )
    }

    fn structured_patch_of(
        lua: &Lua,
        relative: &str,
        format: StructuredFormat,
        priority: Level,
        source: &str,
    ) -> StructuredPatch {
        let apply: Function = lua.load(source).call(()).unwrap();
        StructuredPatch {
            target: Some(Route::new(RouteBase::Home, relative).unwrap()),
            filter: None,
            format,
            apply,
            priority,
        }
    }

    fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
        let apply: Function = lua.load(source).call(()).unwrap();
        RcPatch {
            target: Some(Route::new(RouteBase::Home, relative).unwrap()),
            filter: None,
            apply,
            priority,
        }
    }

    fn placed_structured(relative: &str) -> (Route, Data) {
        (
            Route::new(RouteBase::Home, relative).unwrap(),
            Data::Structured {
                format: StructuredFormat::Json,
                data: serde_json::json!({"seed": 0}),
            },
        )
    }

    fn placed_rc(relative: &str) -> (Route, Data) {
        (
            Route::new(RouteBase::Home, relative).unwrap(),
            Data::Rc(RcData::new(Vec::new(), Vec::new(), Vec::new())),
        )
    }

    fn placed_text(relative: &str) -> (Route, Data) {
        (
            Route::new(RouteBase::Home, relative).unwrap(),
            Data::Text {
                content: "hi".to_owned(),
                mode: 0o644,
                unmanaged: false,
            },
        )
    }

    fn structured_filter_of(lua: &Lua, filter_source: &str, source: &str) -> OwnedPatch {
        let filter: Function = lua.load(filter_source).call(()).unwrap();
        let apply: Function = lua.load(source).call(()).unwrap();
        OwnedPatch::join_patch(
            Patch::Structured(StructuredPatch {
                target: None,
                filter: Some(filter),
                format: StructuredFormat::Json,
                apply,
                priority: Level::Normal,
            }),
            "tools/test",
        )
    }

    fn rc_filter_of(lua: &Lua, filter_source: &str, source: &str) -> OwnedPatch {
        let filter: Function = lua.load(filter_source).call(()).unwrap();
        let apply: Function = lua.load(source).call(()).unwrap();
        OwnedPatch::join_patch(
            Patch::Rc(RcPatch {
                target: None,
                filter: Some(filter),
                apply,
                priority: Level::Normal,
            }),
            "tools/test",
        )
    }

    #[test]
    fn split_patches_keeps_return_order_per_side() {
        let lua = Lua::new();
        let first = structured_patch_of(
            &lua,
            "one.json",
            StructuredFormat::Json,
            Level::Normal,
            r#"return function(document) return document end"#,
        );
        let second = structured_patch_of(
            &lua,
            "two.json",
            StructuredFormat::Json,
            Level::Normal,
            r#"return function(document) return document end"#,
        );
        let third = rc_patch_of(
            &lua,
            "first.sh",
            Level::Normal,
            r#"return function(document) return document end"#,
        );
        let fourth = rc_patch_of(
            &lua,
            "second.sh",
            Level::Normal,
            r#"return function(document) return document end"#,
        );
        let profile = profile_of(
            vec![],
            vec![
                Patch::Structured(first),
                Patch::Rc(third),
                Patch::Structured(second),
                Patch::Rc(fourth),
            ],
        );
        let owned = split_patches(&profile);
        assert_eq!(owned.len(), 4);
        for item in &owned {
            assert_eq!(item.owner, "tools/test");
        }
        let displays: Vec<String> = owned
            .iter()
            .map(|item| {
                let target = match &item.patch {
                    Patch::Structured(found) => &found.target,
                    Patch::Rc(found) => &found.target,
                    Patch::Hook(_) => panic!("document patches hold routes"),
                };
                match target {
                    Some(route) => route.display(),
                    None => panic!("target means route"),
                }
            })
            .collect();
        assert_eq!(
            displays,
            vec![
                "home('one.json')".to_owned(),
                "home('first.sh')".to_owned(),
                "home('two.json')".to_owned(),
                "home('second.sh')".to_owned(),
            ]
        );
    }

    #[test]
    fn structured_filters_fan_out_in_destination_order() {
        let lua = Lua::new();
        let placed: BTreeMap<Route, Data> = [
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
            placed_rc(".bashrc"),
            placed_text("note.txt"),
        ]
        .into_iter()
        .collect();
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return string.find(route.relative, "json") ~= nil end"#,
            r#"return function(route, document) return end"#,
        );
        match grouped_structured(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                let displays: Vec<String> = grouped.keys().map(|route| route.display()).collect();
                assert_eq!(
                    displays,
                    vec![
                        "home('apple.json')".to_owned(),
                        "home('zebra.json')".to_owned()
                    ]
                );
            }
            Err(error) => panic!("filter groups: {error}"),
        }
    }

    #[test]
    fn absent_structured_matches_cover_all_of_kind() {
        let lua = Lua::new();
        let placed: BTreeMap<Route, Data> = [
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
            placed_rc(".bashrc"),
        ]
        .into_iter()
        .collect();
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return true end"#,
            r#"return function(route, document) return end"#,
        );
        match grouped_structured(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                assert_eq!(grouped.len(), 2);
                assert!(
                    grouped
                        .keys()
                        .all(|route| route.display().contains(".json"))
                );
            }
            Err(error) => panic!("default filter groups: {error}"),
        }
    }

    #[test]
    fn rc_filters_fan_out_in_destination_order() {
        let lua = Lua::new();
        let placed: BTreeMap<Route, Data> = [
            placed_rc("zebra.sh"),
            placed_rc("apple.sh"),
            placed_structured("config.json"),
        ]
        .into_iter()
        .collect();
        let patch = rc_filter_of(
            &lua,
            r#"return function(route) return string.find(route.relative, "sh") ~= nil end"#,
            r#"return function(route, document) return end"#,
        );
        match grouped_rc(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                let displays: Vec<String> = grouped.keys().map(|route| route.display()).collect();
                assert_eq!(
                    displays,
                    vec!["home('apple.sh')".to_owned(), "home('zebra.sh')".to_owned()]
                );
            }
            Err(error) => panic!("rc filter groups: {error}"),
        }
    }

    #[test]
    fn false_structured_filters_keep_documents_out() {
        let lua = Lua::new();
        let placed: BTreeMap<Route, Data> = [
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
        ]
        .into_iter()
        .collect();
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return false end"#,
            r#"return function(route, document) return end"#,
        );
        match grouped_structured(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                assert!(grouped.is_empty(), "false keeps every document out");
            }
            Err(error) => panic!("false filter groups: {error}"),
        }
    }

    #[test]
    fn failing_structured_filters_fail() {
        let lua = Lua::new();
        let placed: BTreeMap<Route, Data> =
            [placed_structured("config.json")].into_iter().collect();
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) error("filter boom") end"#,
            r#"return function(route, document) return end"#,
        );
        match grouped_structured(&lua, &placed, &[patch]) {
            Ok(_) => panic!("failing filter passes"),
            Err(error) => {
                let text = error.to_string();
                assert!(text.contains("filter boom"), "carries the cause: {text}");
            }
        }
    }
}
