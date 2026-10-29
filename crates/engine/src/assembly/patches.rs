//! Group structured with rc patches over matched destinations.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use mlua::Lua;

use crate::api::patch::{FilterArgs, Patch, PatchTarget, StructuredPatch, join_destination};
use crate::api::profile::Profile;
use confit_binding::callback::Callback;
use confit_model::document::{Data, Document, RcData, StructuredFormat};
use confit_model::routes::Route;

use super::owned::{OwnedPatch, assemble_config};
use confit_core::error::{
    ContextualCause, ContextualResult, DocumentError, Error, EvalError, TerminalResult, contextual,
    error, kind::ErrorKind,
};
use confit_core::suggest;

/// Split one profile's patches into owned patches in return order.
pub(super) fn split_patches(profile: &Profile) -> Vec<OwnedPatch> {
    let mut owned = Vec::new();
    for config in &profile.configs {
        owned.extend(assemble_config(config));
    }
    owned
}

/// Group structured patches over matched destinations.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for doubled destinations, missing
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for doubled destinations, missing
///   destinations, and Lua failures.
pub(super) fn grouped_structured<'a>(
    lua: &Lua,
    placed: &[Document],
    patches: &'a [OwnedPatch],
) -> TerminalResult<Vec<(Route, Vec<&'a OwnedPatch>)>> {
    let mut grouped: Vec<(Route, Vec<&OwnedPatch>)> = Vec::new();
    let mut index: HashMap<Route, usize> = HashMap::new();
    for patch in patches {
        let Some(found) = patch.structured() else {
            continue;
        };
        let target = match join_destination(found.target.clone(), found.filter.clone()) {
            Ok(target) => target,
            Err(_) => {
                return error(ErrorKind::Evaluate, contextual(EvalError::LuaFailed));
            }
        };
        for destination in expanded_destinations(lua, placed, &target, is_structured)? {
            push_grouped(&mut grouped, &mut index, destination, patch);
        }
    }
    for group in grouped.iter_mut().map(|(_, group)| group) {
        group.sort_by_key(|patch| Reverse(rank_of(patch)));
    }
    Ok(grouped)
}

/// Group rc patches over matched destinations.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for doubled destinations, missing
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for doubled destinations, missing
///   destinations, and Lua failures.
pub(super) fn grouped_rc<'a>(
    lua: &Lua,
    placed: &[Document],
    patches: &'a [OwnedPatch],
) -> TerminalResult<Vec<(Route, Vec<&'a OwnedPatch>)>> {
    let mut grouped: Vec<(Route, Vec<&OwnedPatch>)> = Vec::new();
    let mut index: HashMap<Route, usize> = HashMap::new();
    for patch in patches {
        let Some(found) = patch.rc() else {
            continue;
        };
        let target = match join_destination(found.target.clone(), found.filter.clone()) {
            Ok(target) => target,
            Err(_) => {
                return error(ErrorKind::Evaluate, contextual(EvalError::LuaFailed));
            }
        };
        for destination in expanded_destinations(lua, placed, &target, is_rc)? {
            push_grouped(&mut grouped, &mut index, destination, patch);
        }
    }
    for group in grouped.iter_mut().map(|(_, group)| group) {
        group.sort_by_key(|patch| Reverse(rank_of(patch)));
    }
    Ok(grouped)
}

/// Push one patch onto its destination group in first-seen order.
fn push_grouped<'a>(
    grouped: &mut Vec<(Route, Vec<&'a OwnedPatch>)>,
    index: &mut HashMap<Route, usize>,
    destination: Route,
    patch: &'a OwnedPatch,
) {
    match index.get(&destination) {
        Some(position) => grouped[*position].1.push(patch),
        None => {
            index.insert(destination.clone(), grouped.len());
            grouped.push((destination, vec![patch]));
        }
    }
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
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for Lua failures.
fn expanded_destinations(
    lua: &Lua,
    placed: &[Document],
    target: &PatchTarget,
    area_matches: fn(&Document) -> bool,
) -> TerminalResult<Vec<Route>> {
    match target {
        PatchTarget::Target(route) => Ok(vec![route.clone()]),
        PatchTarget::Filter(filter) => matched_destinations(lua, placed, filter, area_matches),
    }
}

/// Collect filter destinations over callback matches.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for Lua failures.
fn matched_destinations(
    lua: &Lua,
    placed: &[Document],
    filter: &Callback<FilterArgs>,
    area_matches: fn(&Document) -> bool,
) -> TerminalResult<Vec<Route>> {
    let mut matched = Vec::new();
    for document in placed {
        if !area_matches(document) {
            continue;
        }
        let keep = match filter.call::<bool>(lua, (&document.path,)) {
            Ok(found) => found,
            Err(fault) => {
                if let Some(terminal) = fault.downcast_ref::<Error>() {
                    return Err(terminal.clone());
                }
                return error(ErrorKind::LuaFailed, fault);
            }
        };
        if keep {
            matched.push(document.path.clone());
        }
    }
    Ok(matched)
}

/// Report whether documents hold structured data.
fn is_structured(document: &Document) -> bool {
    matches!(document.data, Data::Structured { .. })
}

/// Report whether documents hold rc sections.
fn is_rc(document: &Document) -> bool {
    matches!(document.data, Data::Rc(_))
}

/// Build one structured patch base with its live data.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::PatchTargetUnknown`] for missing targets.
/// - [`confit_core::error::DocumentError::RenderFailed`] for foreign bases with gaps.
pub(super) fn structured_base_of(
    placed: &[Document],
    target: &Route,
    patches: &[&StructuredPatch],
) -> ContextualResult<(StructuredFormat, BTreeMap<String, serde_json::Value>)> {
    match placed.iter().find(|document| document.path == *target) {
        Some(document) => structured_declared_base(&document.data, patches),
        None => Err(unknown_target(target, placed)),
    }
}

/// Build one declared structured patch base with its live data.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] for foreign bases with gaps.
fn structured_declared_base(
    body: &Data,
    patches: &[&StructuredPatch],
) -> ContextualResult<(StructuredFormat, BTreeMap<String, serde_json::Value>)> {
    match body {
        Data::Structured(found) => {
            for patch in patches {
                if patch.format != found.format {
                    return Err(contextual(DocumentError::RenderFailed));
                }
            }
            Ok((found.format, found.data.clone()))
        }
        Data::Rc(_) => Err(contextual(DocumentError::RenderFailed)),
        Data::Text(_) => Err(contextual(DocumentError::RenderFailed)),
        Data::Link(_) => Err(contextual(DocumentError::RenderFailed)),
        Data::Opaque(_) => Err(contextual(DocumentError::RenderFailed)),
        Data::Tree(_) => Err(contextual(DocumentError::RenderFailed)),
    }
}

/// Build one rc patch base with its live sections.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::PatchTargetUnknown`] for missing targets.
/// - [`confit_core::error::DocumentError::RenderFailed`] for foreign bases with gaps.
pub(super) fn rc_base_of(placed: &[Document], target: &Route) -> ContextualResult<RcData> {
    match placed.iter().find(|document| document.path == *target) {
        Some(document) => match &document.data {
            Data::Rc(found) => Ok(found.clone()),
            Data::Structured(_)
            | Data::Text(_)
            | Data::Link(_)
            | Data::Opaque(_)
            | Data::Tree(_) => Err(contextual(DocumentError::RenderFailed)),
        },
        None => Err(unknown_target(target, placed)),
    }
}

/// Build one missing patch target error naming near documents.
fn unknown_target(target: &Route, placed: &[Document]) -> ContextualCause {
    let wanted = target.display();
    let candidates: Vec<String> = placed
        .iter()
        .map(|document| document.path.display())
        .collect();
    let suggestions = suggest::nearby(&wanted, &candidates);
    contextual(EvalError::PatchTargetUnknown)
        .with_document(wanted)
        .with_suggestions(suggestions)
        .with_trace(target.trace.0.iter().map(|site| site.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::config::Config;
    use crate::api::patch::{Level, Patch, RcPatch, StructuredPatch};
    use confit_model::document::{StructuredDocument, TextDocument};
    use confit_model::id::Id;
    use confit_model::routes::RouteBase;
    use mlua::{Function, Lua};

    /// Build one profile with declared documents and patches.
    fn profile_of(documents: Vec<Document>, patches: Vec<Patch>) -> Profile {
        Profile::new(
            vec![Config::new(
                Id::of("tools/test"),
                documents,
                patches,
                Vec::new(),
                std::collections::HashMap::new(),
            )],
            Vec::new(),
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
            apply: Callback::new(apply),
            priority,
        }
    }

    fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
        let apply: Function = lua.load(source).call(()).unwrap();
        RcPatch {
            target: Some(Route::new(RouteBase::Home, relative).unwrap()),
            filter: None,
            apply: Callback::new(apply),
            priority,
        }
    }

    /// Build one seeded json document with its destination path.
    fn placed_structured(relative: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data: BTreeMap::from([("seed".to_owned(), serde_json::json!(0))]),
            }),
        )
    }

    /// Build one empty rc document with its destination path.
    fn placed_rc(relative: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            Data::Rc(RcData::new(Vec::new(), Vec::new(), Vec::new())),
        )
    }

    /// Build one text document with its destination path.
    fn placed_text(relative: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            Data::Text(TextDocument {
                content: "hi".to_owned(),
                mode: 0o644,
                unmanaged: false,
            }),
        )
    }

    fn structured_filter_of(lua: &Lua, filter_source: &str, source: &str) -> OwnedPatch {
        let filter: Function = lua.load(filter_source).call(()).unwrap();
        let apply: Function = lua.load(source).call(()).unwrap();
        OwnedPatch::join_patch(
            Patch::Structured(StructuredPatch {
                target: None,
                filter: Some(Callback::new(filter)),
                format: StructuredFormat::Json,
                apply: Callback::new(apply),
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
                filter: Some(Callback::new(filter)),
                apply: Callback::new(apply),
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
        let placed = vec![
            placed_structured("apple.json"),
            placed_structured("zebra.json"),
            placed_rc(".bashrc"),
            placed_text("note.txt"),
        ];
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return string.find(route:relative(), "json") ~= nil end"#,
            r#"return function(document) return end"#,
        );
        match grouped_structured(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                let displays: Vec<String> =
                    grouped.iter().map(|(route, _)| route.display()).collect();
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
        let placed = vec![
            placed_structured("apple.json"),
            placed_structured("zebra.json"),
            placed_rc(".bashrc"),
        ];
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return true end"#,
            r#"return function(document) return end"#,
        );
        match grouped_structured(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                assert_eq!(grouped.len(), 2);
                assert!(
                    grouped
                        .iter()
                        .all(|(route, _)| route.display().contains(".json"))
                );
            }
            Err(error) => panic!("default filter groups: {error}"),
        }
    }

    #[test]
    fn rc_filters_fan_out_in_destination_order() {
        let lua = Lua::new();
        let placed = vec![
            placed_rc("apple.sh"),
            placed_rc("zebra.sh"),
            placed_structured("config.json"),
        ];
        let patch = rc_filter_of(
            &lua,
            r#"return function(route) return string.find(route:relative(), "sh") ~= nil end"#,
            r#"return function(document) return end"#,
        );
        match grouped_rc(&lua, &placed, &[patch]) {
            Ok(grouped) => {
                let displays: Vec<String> =
                    grouped.iter().map(|(route, _)| route.display()).collect();
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
        let placed = vec![
            placed_structured("zebra.json"),
            placed_structured("apple.json"),
        ];
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return false end"#,
            r#"return function(document) return end"#,
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
        let placed = vec![placed_structured("config.json")];
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) error("filter boom") end"#,
            r#"return function(document) return end"#,
        );
        assert!(
            grouped_structured(&lua, &placed, &[patch]).is_err(),
            "failing filter fails"
        );
    }

    #[test]
    fn target_groups_follow_first_seen_declaration_order() {
        let lua = Lua::new();
        let placed = vec![
            placed_structured("apple.json"),
            placed_structured("zebra.json"),
        ];
        let zebra = OwnedPatch::join_patch(
            Patch::Structured(structured_patch_of(
                &lua,
                "zebra.json",
                StructuredFormat::Json,
                Level::Normal,
                r#"return function(document) return document end"#,
            )),
            "tools/test",
        );
        let apple = OwnedPatch::join_patch(
            Patch::Structured(structured_patch_of(
                &lua,
                "apple.json",
                StructuredFormat::Json,
                Level::Normal,
                r#"return function(document) return document end"#,
            )),
            "tools/test",
        );
        match grouped_structured(&lua, &placed, &[zebra, apple]) {
            Ok(grouped) => {
                let displays: Vec<String> =
                    grouped.iter().map(|(route, _)| route.display()).collect();
                assert_eq!(
                    displays,
                    vec![
                        "home('zebra.json')".to_owned(),
                        "home('apple.json')".to_owned()
                    ]
                );
            }
            Err(error) => panic!("target groups: {error}"),
        }
    }
}
