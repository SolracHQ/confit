//! Split patch lists.
//! Group structured with rc patches under target routes.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use confit_model::document::{StructuredFormat, Table};
use confit_model::routes::Route;
use confit_surface_api::{Document, Patch, Profile, RcDocument, RcPatch, StructuredPatch};

use crate::error::{EngineError, Result};

/// Split one profile's patches into structured and rc lists in return order.
pub(super) fn split_patches(profile: &Profile) -> (Vec<StructuredPatch>, Vec<RcPatch>) {
    let mut structured = Vec::new();
    let mut rc = Vec::new();
    for config in profile.configs() {
        for patch in config.patches() {
            match patch {
                Patch::Structured(found) => structured.push(found.clone()),
                Patch::Rc(found) => rc.push(found.clone()),
            }
        }
    }
    (structured, rc)
}

/// Group structured patches under target routes in rank-descending level
/// order.
///
/// - Declaration order follows the input vecs, and the stable sort keeps it for
///   equal levels.
pub(super) fn grouped_structured(
    patches: &[StructuredPatch],
) -> BTreeMap<Route, Vec<&StructuredPatch>> {
    let mut grouped: BTreeMap<Route, Vec<&StructuredPatch>> = BTreeMap::new();
    for patch in patches {
        grouped
            .entry(patch.target().clone())
            .or_default()
            .push(patch);
    }
    for group in grouped.values_mut() {
        group.sort_by_key(|patch| Reverse(patch.priority().rank()));
    }
    grouped
}

/// Group rc patches under target routes in rank-descending level order.
///
/// - Declaration order follows the input vecs, and the stable sort keeps it for
///   equal levels.
pub(super) fn grouped_rc(patches: &[RcPatch]) -> BTreeMap<Route, Vec<&RcPatch>> {
    let mut grouped: BTreeMap<Route, Vec<&RcPatch>> = BTreeMap::new();
    for patch in patches {
        grouped
            .entry(patch.target().clone())
            .or_default()
            .push(patch);
    }
    for group in grouped.values_mut() {
        group.sort_by_key(|patch| Reverse(patch.priority().rank()));
    }
    grouped
}

/// Build one structured patch base with its destination and live data.
///
/// - Missing targets seed empty maps.
///
/// # Errors
///
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
        None => structured_seeded_base(&target.display(), first, patches),
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
                if patch.format() != found.format {
                    return Err(gap(display, found.format.name(), patch.format().name()));
                }
            }
            let mut data = Table::new();
            for (key, value) in &found.data {
                data.insert(key.clone(), value.0.clone());
            }
            Ok((destination.clone(), found.format, data))
        }
        Document::Rc(_) => Err(gap(display, "rc", first.format().name())),
        _ => Err(gap(display, body.kind_name(), first.format().name())),
    }
}

/// Build one seeded structured patch base under its patch target.
///
/// - Missing targets seed empty maps.
///
/// # Errors
///
/// - [`EngineError::Structured`] for mixed patch groups.
fn structured_seeded_base(
    display: &str,
    first: &StructuredPatch,
    patches: &[&StructuredPatch],
) -> Result<(Route, StructuredFormat, Table)> {
    let format = first.format();
    for patch in patches {
        if patch.format() != format {
            return Err(gap(display, format.name(), patch.format().name()));
        }
    }
    Ok((first.target().clone(), format, Table::new()))
}

/// Build one rc patch base with its destination and live sections.
///
/// - Missing targets seed empty sections.
///
/// # Errors
///
/// - [`EngineError::Structured`] for foreign bases with gaps.
pub(super) fn rc_base_of(
    placed: &BTreeMap<Route, Document>,
    target: &Route,
    first: &RcPatch,
) -> Result<(Route, RcDocument)> {
    match placed.get(target) {
        Some(body) => match body {
            Document::Rc(found) => Ok((target.clone(), found.clone())),
            Document::Structured(found) => Err(gap(&target.display(), found.format.name(), "rc")),
            _ => Err(gap(&target.display(), body.kind_name(), "rc")),
        },
        None => Ok((first.target().clone(), RcDocument::default())),
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
    use confit_surface_api::{Config, Level};
    use mlua::{Function, Lua};

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
            Route::new(RouteBase::Home, relative).unwrap(),
            format,
            apply,
            priority,
            None,
        )
    }

    fn rc_patch_of(lua: &Lua, relative: &str, priority: Level, source: &str) -> RcPatch {
        let apply: Function = lua.load(source).call(()).unwrap();
        RcPatch::new(
            Route::new(RouteBase::Home, relative).unwrap(),
            apply,
            priority,
            None,
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
        let (structured, rc) = split_patches(&profile);
        assert_eq!(structured.len(), 2);
        assert_eq!(structured[0].target().display(), "home('one.json')");
        assert_eq!(structured[1].target().display(), "home('two.json')");
        assert_eq!(rc.len(), 2);
        assert_eq!(rc[0].target().display(), "home('first.sh')");
        assert_eq!(rc[1].target().display(), "home('second.sh')");
    }
}
