//! Split patch lists.
//! Group structured with rc patches over matched destinations.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use mlua::{Function, Lua};

use confit_model::document::{StructuredFormat, Table};
use confit_model::routes::Route;
use confit_surface_api::{
    Document, Patch, PatchTarget, Profile, RcDocument, RcPatch, StructuredPatch, route_value,
};

use crate::error::{EngineError, Result};

/// Split one profile's patches into structured and rc lists in return order.
pub(super) fn split_patches(profile: &Profile) -> (Vec<StructuredPatch>, Vec<RcPatch>) {
    let mut structured = Vec::new();
    let mut rc = Vec::new();
    for config in &profile.configs {
        for patch in &config.patches {
            match patch {
                Patch::Structured(found) => structured.push(found.clone()),
                Patch::Rc(found) => rc.push(found.clone()),
            }
        }
    }
    (structured, rc)
}

/// Group structured patches over matched destinations in rank order.
///
/// - Target patches group under their route.
/// - Filter patches fan out over structured matches.
/// - Fan-out follows destination order.
/// - Declaration order follows the input vecs, and the stable sort
///   keeps it for equal levels.
///
/// # Errors
///
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub(super) fn grouped_structured<'a>(
    lua: &Lua,
    placed: &BTreeMap<Route, Document>,
    patches: &'a [StructuredPatch],
) -> Result<BTreeMap<Route, Vec<&'a StructuredPatch>>> {
    let mut grouped: BTreeMap<Route, Vec<&StructuredPatch>> = BTreeMap::new();
    for patch in patches {
        for destination in expanded_destinations(lua, placed, &patch.target, is_structured)? {
            grouped.entry(destination).or_default().push(patch);
        }
    }
    for group in grouped.values_mut() {
        group.sort_by_key(|patch| Reverse(patch.priority.rank()));
    }
    Ok(grouped)
}

/// Group rc patches over matched destinations in rank order.
///
/// - Target patches group under their route.
/// - Filter patches fan out over rc matches.
/// - Fan-out follows destination order.
/// - Declaration order follows the input vecs, and the stable sort
///   keeps it for equal levels.
///
/// # Errors
///
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub(super) fn grouped_rc<'a>(
    lua: &Lua,
    placed: &BTreeMap<Route, Document>,
    patches: &'a [RcPatch],
) -> Result<BTreeMap<Route, Vec<&'a RcPatch>>> {
    let mut grouped: BTreeMap<Route, Vec<&RcPatch>> = BTreeMap::new();
    for patch in patches {
        for destination in expanded_destinations(lua, placed, &patch.target, is_rc)? {
            grouped.entry(destination).or_default().push(patch);
        }
    }
    for group in grouped.values_mut() {
        group.sort_by_key(|patch| Reverse(patch.priority.rank()));
    }
    Ok(grouped)
}

/// Collect patch destinations over its target.
///
/// # Errors
///
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
fn expanded_destinations(
    lua: &Lua,
    placed: &BTreeMap<Route, Document>,
    target: &PatchTarget,
    area_matches: fn(&Document) -> bool,
) -> Result<Vec<Route>> {
    match target {
        PatchTarget::Target(route) => Ok(vec![route.clone()]),
        PatchTarget::Filter(filter) => {
            matched_destinations(lua, placed, filter.as_function(), area_matches)
        }
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
    placed: &BTreeMap<Route, Document>,
    filter: &Function,
    area_matches: fn(&Document) -> bool,
) -> Result<Vec<Route>> {
    let mut matched = Vec::new();
    for (destination, body) in placed {
        if !area_matches(body) {
            continue;
        }
        let route = route_value(lua, destination)?;
        let keep: bool = filter.call(route)?;
        if keep {
            matched.push(destination.clone());
        }
    }
    Ok(matched)
}

/// Report whether one document holds structured data.
fn is_structured(body: &Document) -> bool {
    matches!(body, Document::Structured(_))
}

/// Report whether one document holds rc sections.
fn is_rc(body: &Document) -> bool {
    matches!(body, Document::Rc(_))
}

/// Build one structured patch base with its destination and live data.
///
/// # Errors
///
/// - [`EngineError::UnknownTarget`] for missing targets.
/// - [`EngineError::Structured`] for foreign bases with gaps.
pub(super) fn structured_base_of(
    placed: &BTreeMap<Route, Document>,
    target: &Route,
    first: &StructuredPatch,
    patches: &[&StructuredPatch],
) -> Result<(Route, StructuredFormat, Table)> {
    match placed.get(target) {
        Some(body) => {
            let display = target.display();
            structured_declared_base(target, body, &display, first, patches)
        }
        None => Err(EngineError::UnknownTarget {
            display: target.display(),
        }),
    }
}

/// Build one declared structured patch base with its destination and live data.
///
/// # Errors
///
/// - [`EngineError::Structured`] for foreign bases with gaps.
fn structured_declared_base(
    destination: &Route,
    body: &Document,
    display: &str,
    first: &StructuredPatch,
    patches: &[&StructuredPatch],
) -> Result<(Route, StructuredFormat, Table)> {
    match body {
        Document::Structured(found) => {
            for patch in patches {
                if patch.format != found.format {
                    return Err(gap(display, found.format.name(), patch.format.name()));
                }
            }
            let mut data = Table::new();
            for (key, value) in &found.data {
                data.insert(key.clone(), value.0.clone());
            }
            Ok((destination.clone(), found.format, data))
        }
        Document::Rc(_) => Err(gap(display, "rc", first.format.name())),
        _ => Err(gap(display, body.kind_name(), first.format.name())),
    }
}

/// Build one rc patch base with its destination and live sections.
///
/// # Errors
///
/// - [`EngineError::UnknownTarget`] for missing targets.
/// - [`EngineError::Structured`] for foreign bases with gaps.
pub(super) fn rc_base_of(
    placed: &BTreeMap<Route, Document>,
    target: &Route,
) -> Result<(Route, RcDocument)> {
    match placed.get(target) {
        Some(body) => match body {
            Document::Rc(found) => Ok((target.clone(), found.clone())),
            Document::Structured(found) => Err(gap(&target.display(), found.format.name(), "rc")),
            _ => Err(gap(&target.display(), body.kind_name(), "rc")),
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
    use confit_model::routes::RouteBase;
    use confit_surface_api::{
        Config, Level, Mode, SerializableValue, StructuredDocument, TextDocument,
    };
    use mlua::{Function, Lua};
    use std::collections::BTreeMap;

    fn profile_of(documents: Vec<(Route, Document)>, patches: Vec<Patch>) -> Profile {
        Profile::new(
            Vec::new(),
            vec![Config::new(
                "tools/test".to_owned(),
                documents.into_iter().collect(),
                patches,
                Vec::new(),
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
        StructuredPatch::new(
            PatchTarget::Target(Route::new(RouteBase::Home, relative).unwrap()),
            format,
            apply,
            priority,
            None,
        )
    }

    fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
        let apply: Function = lua.load(source).call(()).unwrap();
        RcPatch::new(
            PatchTarget::Target(Route::new(RouteBase::Home, relative).unwrap()),
            apply,
            priority,
            None,
        )
    }

    fn placed_structured(relative: &str) -> (Route, Document) {
        let body = StructuredDocument {
            format: StructuredFormat::Json,
            data: [(
                "seed".to_owned(),
                SerializableValue(serde_json::Value::from(0)),
            )]
            .into_iter()
            .collect(),
        };
        (
            Route::new(RouteBase::Home, relative).unwrap(),
            Document::Structured(body),
        )
    }

    fn placed_rc(relative: &str) -> (Route, Document) {
        (
            Route::new(RouteBase::Home, relative).unwrap(),
            Document::Rc(RcDocument {
                profile: Vec::new(),
                config: Vec::new(),
                final_entries: Vec::new(),
            }),
        )
    }

    fn placed_text(relative: &str) -> (Route, Document) {
        (
            Route::new(RouteBase::Home, relative).unwrap(),
            Document::Text(TextDocument {
                text: "hi".to_owned(),
                mode: Mode::default(),
                unmanaged: None,
            }),
        )
    }

    fn structured_filter_of(lua: &Lua, filter_source: &str, source: &str) -> StructuredPatch {
        let filter: Function = lua.load(filter_source).call(()).unwrap();
        let apply: Function = lua.load(source).call(()).unwrap();
        StructuredPatch::new(
            PatchTarget::from_filter(filter),
            StructuredFormat::Json,
            apply,
            Level::Normal,
            None,
        )
    }

    fn rc_filter_of(lua: &Lua, filter_source: &str, source: &str) -> RcPatch {
        let filter: Function = lua.load(filter_source).call(()).unwrap();
        let apply: Function = lua.load(source).call(()).unwrap();
        RcPatch::new(PatchTarget::from_filter(filter), apply, Level::Normal, None)
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
        let (structured, rc) = split_patches(&profile);
        assert_eq!(structured.len(), 2);
        for (patch, want) in [
            (&structured[0], "home('one.json')"),
            (&structured[1], "home('two.json')"),
        ] {
            let route = match &patch.target {
                PatchTarget::Target(route) => route,
                PatchTarget::Filter(_) => panic!("target means route"),
            };
            assert_eq!(route.display(), want);
        }
        assert_eq!(rc.len(), 2);
        for (patch, want) in [(&rc[0], "home('first.sh')"), (&rc[1], "home('second.sh')")] {
            let route = match &patch.target {
                PatchTarget::Target(route) => route,
                PatchTarget::Filter(_) => panic!("target means route"),
            };
            assert_eq!(route.display(), want);
        }
    }

    #[test]
    fn structured_filters_fan_out_in_destination_order() {
        let lua = Lua::new();
        let placed: BTreeMap<Route, Document> = [
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
        let placed: BTreeMap<Route, Document> = [
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
        let placed: BTreeMap<Route, Document> = [
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
        let placed: BTreeMap<Route, Document> = [
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
        let placed: BTreeMap<Route, Document> =
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
