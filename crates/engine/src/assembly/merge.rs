//! Merges patch callbacks.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};

use crate::api::patch::{RcWorkingDocument, StructuredPatch, StructuredWorkingDocument};
use confit_model::document::{
    Data, Document, RcData, RcEntry, StructuredDocument, StructuredFormat,
};
use confit_model::routes::Route;
use lua_binding::value::{Decode, Encode};

use super::Assembler;
use super::owned::OwnedPatch;
use super::patches::{rc_base_of, structured_base_of};
use confit_core::error::{
    DocumentError, Error, ErrorCause, EvalError, TerminalResult, contextual, error, kind::ErrorKind,
};

impl Assembler<'_> {
    /// Rebuilds the structured bases.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a target is
    ///   missing.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a base
    ///   holds another shape.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
    ///   fails, a shape is bad, or a path drifts.
    pub(super) fn apply_structured(
        &self,
        placed: &[Document],
        grouped: &[(Route, Vec<&OwnedPatch>)],
        finished: &mut Vec<Document>,
    ) -> TerminalResult<()> {
        for (target, patches) in grouped {
            let inners: Vec<&StructuredPatch> = patches
                .iter()
                .filter_map(|patch| patch.structured())
                .collect();
            if inners.is_empty() {
                continue;
            };
            let display = target.display();
            let (format, data) = match structured_base_of(placed, target, &inners) {
                Ok(found) => found,
                Err(contextual) => match contextual.cause() {
                    ErrorCause::EvalError(EvalError::PatchTargetUnknown) => {
                        return error(ErrorKind::Evaluate, contextual);
                    }
                    _ => {
                        return error(
                            ErrorKind::RenderManifest,
                            contextual.with_document(target.display()),
                        );
                    }
                },
            };
            let data = self.run_callbacks(target, &display, &format, data, patches)?;
            finished.push(Document::new(
                target.clone(),
                Data::Structured(StructuredDocument { format, data }),
            ));
        }
        Ok(())
    }

    /// Rebuilds the rc bases.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a target is
    ///   missing.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a base
    ///   holds another shape.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
    ///   fails, a shape is bad, or a path drifts.
    pub(super) fn apply_rc(
        &self,
        placed: &[Document],
        grouped: &[(Route, Vec<&OwnedPatch>)],
        finished: &mut Vec<Document>,
    ) -> TerminalResult<()> {
        for (target, patches) in grouped {
            let document = match rc_base_of(placed, target) {
                Ok(found) => found,
                Err(contextual) => match contextual.cause() {
                    ErrorCause::EvalError(EvalError::PatchTargetUnknown) => {
                        return error(ErrorKind::Evaluate, contextual);
                    }
                    _ => {
                        return error(
                            ErrorKind::RenderManifest,
                            contextual.with_document(target.display()),
                        );
                    }
                },
            };
            let display = target.display();
            let patched = self.run_rc_callbacks(target, &display, document, patches)?;
            let data = RcData::new(patched.profile, patched.config, patched.r#final);
            finished.push(Document::new(target.clone(), Data::Rc(data)));
        }
        Ok(())
    }

    /// Runs the structured callbacks.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a shape is
    ///   bad.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a
    ///   format changes.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
    ///   fails or a path drifts.
    pub(super) fn run_callbacks(
        &self,
        destination: &Route,
        display: &str,
        format: &StructuredFormat,
        data: BTreeMap<String, serde_json::Value>,
        patches: &[&OwnedPatch],
    ) -> TerminalResult<BTreeMap<String, serde_json::Value>> {
        let mut data = data;
        let mut owners = seed_key_owners(&data);
        for patch in patches {
            let Some(found) = patch.structured() else {
                continue;
            };
            let working = StructuredWorkingDocument {
                path: destination.clone(),
                body: StructuredDocument {
                    format: *format,
                    data: data.clone(),
                },
            };
            let table = working_view(self.lua(), &working)?;
            match found.apply.call::<()>(self.lua(), (table.clone(),)) {
                Ok(()) => {}
                Err(fault) => {
                    if let Some(terminal) = fault.downcast_ref::<Error>() {
                        return Err(terminal.clone());
                    }
                    return error(ErrorKind::LuaFailed, fault);
                }
            }
            let back = match StructuredWorkingDocument::decode(&Value::Table(table), self.lua()) {
                Ok(found) => found,
                Err(_) => {
                    return error(
                        ErrorKind::Evaluate,
                        contextual(EvalError::LuaFailed).with_document(display),
                    );
                }
            };
            validate_working_path(destination, &back.path)?;
            if back.body.format != *format {
                return error(
                    ErrorKind::RenderManifest,
                    contextual(DocumentError::RenderFailed).with_document(display),
                );
            }
            data = merge(display, data, &mut owners, back.body.data, &patch.owner);
        }
        Ok(data)
    }

    /// Runs the rc callbacks.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a shape is
    ///   bad.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
    ///   fails or a path drifts.
    pub(super) fn run_rc_callbacks(
        &self,
        destination: &Route,
        display: &str,
        document: RcData,
        patches: &[&OwnedPatch],
    ) -> TerminalResult<RcData> {
        let mut document = document;
        let mut owned = seed_collision_owners(&document);
        for patch in patches {
            let Some(found) = patch.rc() else {
                continue;
            };
            let working = RcWorkingDocument {
                path: destination.clone(),
                body: document.clone(),
            };
            let table = working_view(self.lua(), &working)?;
            match found.apply.call::<()>(self.lua(), (table.clone(),)) {
                Ok(()) => {}
                Err(fault) => {
                    if let Some(terminal) = fault.downcast_ref::<Error>() {
                        return Err(terminal.clone());
                    }
                    return error(ErrorKind::LuaFailed, fault);
                }
            }
            let back = match RcWorkingDocument::decode(&Value::Table(table), self.lua()) {
                Ok(found) => found,
                Err(_) => {
                    return error(
                        ErrorKind::Evaluate,
                        contextual(EvalError::LuaFailed).with_document(display),
                    );
                }
            };
            validate_working_path(destination, &back.path)?;
            document = merge_rc(display, &document, back.body, &mut owned, &patch.owner);
        }
        Ok(document)
    }
}

#[derive(Default)]
struct CollisionOwners {
    /// Collision owners in the profile section.
    profile: BTreeMap<String, String>,
    /// Collision owners in the config section.
    config: BTreeMap<String, String>,
    /// Collision owners in the final section.
    final_entries: BTreeMap<String, String>,
}

/// Encodes one working document.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when the shape falls
///   outside the table shape.
fn working_view(lua: &Lua, document: &impl Encode) -> TerminalResult<Table> {
    match document.encode(lua) {
        Ok(Value::Table(table)) => Ok(table),
        Ok(_) => error(ErrorKind::LuaFailed, contextual(EvalError::LuaFailed)),
        Err(fault) => error(ErrorKind::LuaFailed, fault),
    }
}

/// Rejects a moved working path.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when the path moves
///   off the target.
fn validate_working_path(destination: &Route, back: &Route) -> TerminalResult<()> {
    if back != destination {
        return error(ErrorKind::LuaFailed, contextual(EvalError::LuaFailed));
    }
    Ok(())
}

/// Seeds the key owners.
fn seed_key_owners(data: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, String> {
    let mut owners = BTreeMap::new();
    for (key, value) in data {
        seed_leaves(value, key, "base", &mut owners);
    }
    owners
}

/// Seeds the leaf owners.
fn seed_leaves(
    value: &serde_json::Value,
    path: &str,
    holder: &str,
    owners: &mut BTreeMap<String, String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                let child = format!("{path}.{key}");
                seed_leaves(item, &child, holder, owners);
            }
        }
        _ => {
            owners.insert(path.to_owned(), holder.to_owned());
        }
    }
}

/// Merges patch data over base data.
fn merge(
    display: &str,
    base: BTreeMap<String, serde_json::Value>,
    owners: &mut BTreeMap<String, String>,
    next: BTreeMap<String, serde_json::Value>,
    owner: &str,
) -> BTreeMap<String, serde_json::Value> {
    let loser = owner;
    let mut merged = base;
    let missing: Vec<String> = merged
        .keys()
        .filter(|key| !next.contains_key(*key))
        .cloned()
        .collect();
    for (key, value) in next {
        fold_entry(display, &mut merged, owners, key, value, loser);
    }
    for key in missing {
        if let Some(found) = merged.get(&key).cloned() {
            log_removed(display, &found, &key, owners, loser);
        }
    }
    merged
}

/// Folds one top-level entry.
fn fold_entry(
    display: &str,
    merged: &mut BTreeMap<String, serde_json::Value>,
    owners: &mut BTreeMap<String, String>,
    key: String,
    next_value: serde_json::Value,
    loser: &str,
) {
    let Some(found) = merged.get_mut(&key) else {
        seed_leaves(&next_value, &key, loser, owners);
        merged.insert(key, next_value);
        return;
    };
    if *found == next_value {
        return;
    }
    fold_value(display, &key, found, next_value, owners, loser);
}

/// Folds one patch value over a current value.
fn fold_value(
    display: &str,
    path: &str,
    current: &mut serde_json::Value,
    next: serde_json::Value,
    owners: &mut BTreeMap<String, String>,
    loser: &str,
) {
    if *current == next {
        return;
    }
    let empty = current
        .as_object()
        .map(|map| map.is_empty())
        .unwrap_or(false);
    match (current, next) {
        (serde_json::Value::Object(map), serde_json::Value::Object(next_map)) => {
            fold_maps(display, path, map, next_map, owners, loser);
        }
        (current, found) => {
            if empty {
                seed_leaves(&found, path, loser, owners);
                *current = found;
                return;
            }
            if let Some(winner) = winner_for(path, owners) {
                log::warn!(
                    "collision on {display} \"{path}\": \"{loser}\" loses, \"{winner}\" wins"
                );
            }
        }
    }
}

/// Folds one patch table over the current table.
fn fold_maps(
    display: &str,
    path: &str,
    map: &mut serde_json::Map<String, serde_json::Value>,
    next_map: serde_json::Map<String, serde_json::Value>,
    owners: &mut BTreeMap<String, String>,
    loser: &str,
) {
    let removed: Vec<String> = map
        .keys()
        .filter(|key| !next_map.contains_key(*key))
        .cloned()
        .collect();
    for (key, next_value) in next_map {
        let child = format!("{path}.{key}");
        if let Some(found) = map.get_mut(&key) {
            if *found == next_value {
                continue;
            }
            fold_value(display, &child, found, next_value, owners, loser);
        } else {
            seed_leaves(&next_value, &child, loser, owners);
            map.insert(key, next_value);
        }
    }
    for key in removed {
        if let Some(found) = map.get(&key).cloned() {
            let child = format!("{path}.{key}");
            log_removed(display, &found, &child, owners, loser);
        }
    }
}

/// Logs the removed leaves.
fn log_removed(
    display: &str,
    removed: &serde_json::Value,
    path: &str,
    owners: &BTreeMap<String, String>,
    loser: &str,
) {
    match removed {
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                let child = format!("{path}.{key}");
                log_removed(display, item, &child, owners, loser);
            }
        }
        _ => {
            if let Some(winner) = owners.get(path) {
                log::warn!(
                    "collision on {display} \"{path}\": \"{loser}\" loses, \"{winner}\" wins"
                );
            }
        }
    }
}

/// Returns the winning owner.
fn winner_for(path: &str, owners: &BTreeMap<String, String>) -> Option<String> {
    if let Some(winner) = owners.get(path) {
        return Some(winner.clone());
    }
    let prefix = format!("{path}.");
    for (key, winner) in owners {
        if key.starts_with(&prefix) {
            return Some(winner.clone());
        }
    }
    None
}

/// Seeds the collision owners.
fn seed_collision_owners(document: &RcData) -> CollisionOwners {
    let mut owned = CollisionOwners::default();
    seed_section(&document.profile, &mut owned.profile);
    seed_section(&document.config, &mut owned.config);
    seed_section(&document.r#final, &mut owned.final_entries);
    owned
}

/// Seeds one section.
fn seed_section(entries: &[RcEntry], owned: &mut BTreeMap<String, String>) {
    for entry in entries {
        if let Some(name) = entry.collision_key() {
            owned.insert(name.to_owned(), "base".to_owned());
        }
    }
}

/// Merges patch sections over the current sections.
fn merge_rc(
    display: &str,
    document: &RcData,
    next: RcData,
    owned: &mut CollisionOwners,
    owner: &str,
) -> RcData {
    let loser = owner;
    RcData {
        profile: merge_section(
            display,
            "profile",
            &document.profile,
            next.profile,
            &mut owned.profile,
            loser,
        ),
        config: merge_section(
            display,
            "config",
            &document.config,
            next.config,
            &mut owned.config,
            loser,
        ),
        r#final: merge_section(
            display,
            "final",
            &document.r#final,
            next.r#final,
            &mut owned.final_entries,
            loser,
        ),
    }
}

/// Merges one patch section over its previous entries.
fn merge_section(
    display: &str,
    section: &str,
    previous: &[RcEntry],
    next: Vec<RcEntry>,
    owned: &mut BTreeMap<String, String>,
    loser: &str,
) -> Vec<RcEntry> {
    let mut pool: Vec<&RcEntry> = previous.iter().collect();
    let mut kept = Vec::with_capacity(next.len());
    for entry in next {
        if let Some(position) = pool.iter().position(|item| *item == &entry) {
            pool.remove(position);
            kept.push(entry);
            continue;
        }
        let key = entry.collision_key().map(str::to_owned);
        match key {
            Some(name) => match owned.get(&name) {
                Some(winner) => {
                    log::warn!(
                        "collision on {display} {section} \"{name}\": \"{loser}\" loses, \"{winner}\" wins"
                    );
                }
                None => {
                    owned.insert(name, loser.to_owned());
                    kept.push(entry);
                }
            },
            None => kept.push(entry),
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn distinct_nested_keys_merge_side_by_side() {
        let base: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "latest"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([(
            "tools".to_owned(),
            json!({"bat": "latest", "ripgrep": "latest"}),
        )]);
        let merged = merge("home('config.json')", base, &mut owners, next, "second");
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("latest")
        );
        assert_eq!(
            tools.get("ripgrep").and_then(|item| item.as_str()),
            Some("latest")
        );
        assert_eq!(
            owners.get("tools.ripgrep").map(String::as_str),
            Some("second")
        );
    }

    #[test]
    fn same_leaf_twice_keeps_first_write() {
        let base: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "first"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("tools".to_owned(), json!({"bat": "second"}))]);
        let merged = merge("home('config.json')", base, &mut owners, next, "second");
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("first")
        );
        assert_eq!(owners.get("tools.bat").map(String::as_str), Some("first"));
    }

    #[test]
    fn table_against_scalar_keeps_first_write() {
        let base: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "kept"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("tools".to_owned(), json!("flat"))]);
        let merged = merge("home('config.json')", base, &mut owners, next, "second");
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("kept")
        );
    }

    #[test]
    fn lists_keep_the_first_write_whole() {
        let base: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("items".to_owned(), json!([1, 2]))]);
        let mut owners = BTreeMap::from([("items".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("items".to_owned(), json!([3]))]);
        let merged = merge("home('config.json')", base, &mut owners, next, "second");
        assert_eq!(merged.get("items"), Some(&json!([1, 2])));
        assert_eq!(owners.get("items").map(String::as_str), Some("first"));
    }

    #[test]
    fn seeded_base_leaves_beat_patch_writes() {
        let base: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "base"}))]);
        let mut owners = seed_key_owners(&base);
        let next = BTreeMap::from([(
            "tools".to_owned(),
            json!({"bat": "patch", "ripgrep": "latest"}),
        )]);
        let merged = merge("home('config.json')", base, &mut owners, next, "patch");
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("base")
        );
        assert_eq!(
            tools.get("ripgrep").and_then(|item| item.as_str()),
            Some("latest")
        );
        assert_eq!(owners.get("tools.bat").map(String::as_str), Some("base"));
        assert_eq!(
            owners.get("tools.ripgrep").map(String::as_str),
            Some("patch")
        );
    }
}
