//! Groups patches by destination.

use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap};

use mlua::Lua;

use crate::api::patch::{FilterArgs, Patch, PatchTarget, StructuredPatch, join_destination};
use crate::api::profile::Profile;
use confit_model::document::{Data, Document, RcData, StructuredFormat};
use confit_model::routes::Route;
use lua_binding::callable::Callable;

use super::owned::{OwnedPatch, assemble_config};
use confit_core::error::{
    ContextualCause, ContextualResult, DocumentError, Error, EvalError, TerminalResult, contextual,
    error, kind::ErrorKind,
};
use confit_core::suggest;

/// Splits one profile's patches.
pub(super) fn split_patches(profile: &Profile) -> Vec<OwnedPatch> {
    let mut owned = Vec::new();
    for config in &profile.configs {
        owned.extend(assemble_config(config));
    }
    owned
}

/// Groups the structured patches.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a destination
///   repeats or a destination is missing.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a destination
///   fails to join or a callback fails.
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

/// Groups the rc patches.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a destination
///   repeats or a destination is missing.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a destination
///   fails to join or a callback fails.
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

/// Adds one patch to its destination group.
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

/// Returns the merge rank.
fn rank_of(patch: &OwnedPatch) -> u8 {
    match &patch.patch {
        Patch::Structured(found) => found.priority.rank(),
        Patch::Rc(found) => found.priority.rank(),
        Patch::Hook(_) => 0,
    }
}

/// Collects the patch destinations.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
///   fails.
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

/// Collects the filter destinations.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
///   fails.
fn matched_destinations(
    lua: &Lua,
    placed: &[Document],
    filter: &Callable<FilterArgs>,
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

/// Returns whether documents hold structured data.
fn is_structured(document: &Document) -> bool {
    matches!(document.data, Data::Structured { .. })
}

/// Returns whether documents hold rc sections.
fn is_rc(document: &Document) -> bool {
    matches!(document.data, Data::Rc(_))
}

/// Builds the structured base.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::PatchTargetUnknown`] when the target is
///   missing.
/// - [`confit_core::error::DocumentError::RenderFailed`] when the base holds
///   another shape.
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

/// Builds the declared structured base.
///
/// # Errors
///
/// - [`confit_core::error::DocumentError::RenderFailed`] when the base holds
///   another shape.
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

/// Builds the rc base.
///
/// # Errors
///
/// - [`confit_core::error::EvalError::PatchTargetUnknown`] when the target is
///   missing.
/// - [`confit_core::error::DocumentError::RenderFailed`] when the base holds
///   another shape.
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

/// Builds the unknown target error.
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
    use confit_model::document::{FileMode, StructuredDocument, TextDocument};
    use confit_model::id::Id;
    use confit_model::routes::RouteBase;
    use mlua::{Function, Lua};

    /// Builds one profile.
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
            apply: Callable::new(apply),
            priority,
        }
    }

    fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
        let apply: Function = lua.load(source).call(()).unwrap();
        RcPatch {
            target: Some(Route::new(RouteBase::Home, relative).unwrap()),
            filter: None,
            apply: Callable::new(apply),
            priority,
        }
    }

    /// Builds one seeded structured document.
    fn placed_structured(relative: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            false,
            Data::Structured(StructuredDocument {
                format: StructuredFormat::Json,
                data: BTreeMap::from([("seed".to_owned(), serde_json::json!(0))]),
            }),
        )
    }

    /// Builds one empty rc document.
    fn placed_rc(relative: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            false,
            Data::Rc(RcData::new(Vec::new(), Vec::new(), Vec::new())),
        )
    }

    /// Builds one text document.
    fn placed_text(relative: &str) -> Document {
        Document::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            false,
            Data::Text(TextDocument {
                content: "hi".to_owned(),
                mode: FileMode::new(0o644),
            }),
        )
    }

    fn structured_filter_of(lua: &Lua, filter_source: &str, source: &str) -> OwnedPatch {
        let filter: Function = lua.load(filter_source).call(()).unwrap();
        let apply: Function = lua.load(source).call(()).unwrap();
        OwnedPatch::join_patch(
            Patch::Structured(StructuredPatch {
                target: None,
                filter: Some(Callable::new(filter)),
                format: StructuredFormat::Json,
                apply: Callable::new(apply),
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
                filter: Some(Callable::new(filter)),
                apply: Callable::new(apply),
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
                    Patch::Hook(_) => panic!("expected a document patch"),
                };
                match target {
                    Some(route) => route.display(),
                    None => panic!("expected a target route"),
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
    fn structured_filters_reach_every_match_in_order() {
        let lua = Lua::new();
        let placed = vec![
            placed_structured("apple.json"),
            placed_structured("zebra.json"),
            placed_rc(".bashrc"),
            placed_text("note.txt"),
        ];
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) return string.find(route.relative, "json") ~= nil end"#,
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
            Err(error) => panic!("the structured filter fails to group: {error}"),
        }
    }

    #[test]
    fn absent_structured_matches_cover_every_document_of_the_kind() {
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
            Err(error) => panic!("the default filter fails to group: {error}"),
        }
    }

    #[test]
    fn rc_filters_reach_every_match_in_order() {
        let lua = Lua::new();
        let placed = vec![
            placed_rc("apple.sh"),
            placed_rc("zebra.sh"),
            placed_structured("config.json"),
        ];
        let patch = rc_filter_of(
            &lua,
            r#"return function(route) return string.find(route.relative, "sh") ~= nil end"#,
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
            Err(error) => panic!("the rc filter fails to group: {error}"),
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
                assert!(
                    grouped.is_empty(),
                    "the false filter keeps every document out"
                );
            }
            Err(error) => panic!("the false filter fails to group: {error}"),
        }
    }

    #[test]
    fn structured_filter_rejects_a_callback_error() {
        let lua = Lua::new();
        let placed = vec![placed_structured("config.json")];
        let patch = structured_filter_of(
            &lua,
            r#"return function(route) error("filter boom") end"#,
            r#"return function(document) return end"#,
        );
        assert!(
            grouped_structured(&lua, &placed, &[patch]).is_err(),
            "the structured filter rejects a failing callback"
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
            Err(error) => panic!("the target grouping fails to order: {error}"),
        }
    }
}
