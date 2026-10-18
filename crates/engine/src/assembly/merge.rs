//! Merge patch callbacks under first-writer wins.

use std::collections::BTreeMap;

use mlua::{Lua, Table, Value};

use crate::api::patch::{RcLiveDocument, StructuredLiveDocument, StructuredPatch};
use confit_binding::value::{Decode, Encode};
use confit_model::document::{
    Data, Document, RcData, RcEntry, StructuredDocument, StructuredFormat,
};
use confit_model::routes::Route;

use super::Assembler;
use super::owned::OwnedPatch;
use super::patches::{rc_base_of, structured_base_of};
use crate::error::{EngineError, Result};

impl Assembler<'_> {
    /// Apply structured patch groups over declared bases into documents.
    ///
    /// - First writes win in rank-descending level order with collision logs.
    /// - Groups follow first-seen declaration order.
    /// - Callbacks run over live documents.
    ///
    /// # Errors
    ///
    /// - [`EngineError::UnknownTarget`] for missing targets.
    /// - [`EngineError::Structured`] for foreign bases with gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn apply_structured(
        &self,
        placed: &[Document],
        grouped: &[(Route, Vec<&OwnedPatch>)],
        finished: &mut Vec<Document>,
    ) -> Result<()> {
        for (target, patches) in grouped {
            let inners: Vec<&StructuredPatch> = patches
                .iter()
                .filter_map(|patch| patch.structured())
                .collect();
            let Some(first) = inners.first().copied() else {
                continue;
            };
            let display = target.display();
            let (format, data) = structured_base_of(placed, target, first, &inners)?;
            let data = self.run_callbacks(target, &display, &format, data, patches)?;
            finished.push(Document::new(
                target.clone(),
                Data::Structured(StructuredDocument { format, data }),
            ));
        }
        Ok(())
    }

    /// Apply rc patch groups over declared bases into documents.
    ///
    /// - First writes win in rank-descending level order with collision logs.
    /// - Groups follow first-seen declaration order.
    /// - Callbacks run over live documents.
    ///
    /// # Errors
    ///
    /// - [`EngineError::UnknownTarget`] for missing targets.
    /// - [`EngineError::Structured`] for foreign bases with gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn apply_rc(
        &self,
        placed: &[Document],
        grouped: &[(Route, Vec<&OwnedPatch>)],
        finished: &mut Vec<Document>,
    ) -> Result<()> {
        for (target, patches) in grouped {
            let document = rc_base_of(placed, target)?;
            let display = target.display();
            let patched = self.run_rc_callbacks(target, &display, document, patches)?;
            let data = RcData::new(patched.profile, patched.config, patched.r#final);
            finished.push(Document::new(target.clone(), Data::Rc(data)));
        }
        Ok(())
    }

    /// Run patch callbacks over base data through live documents keeping
    /// first writes.
    ///
    /// - Callbacks run over live documents.
    /// - Callbacks keep the path fixed.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Structured`] for format gaps.
    /// - [`EngineError::Mismatch`] for tables outside the live shape.
    /// - [`EngineError::Lua`] for bridge, callback, and path drift failures.
    pub(super) fn run_callbacks(
        &self,
        destination: &Route,
        display: &str,
        format: &StructuredFormat,
        data: BTreeMap<String, serde_json::Value>,
        patches: &[&OwnedPatch],
    ) -> Result<BTreeMap<String, serde_json::Value>> {
        let mut data = data;
        let mut owners = seed_key_owners(&data);
        for patch in patches {
            let Some(found) = patch.structured() else {
                continue;
            };
            let live = StructuredLiveDocument {
                path: destination.clone(),
                body: StructuredDocument {
                    format: *format,
                    data: data.clone(),
                },
            };
            let table = live_view(self.lua(), &live)?;
            found.apply.call::<()>(self.lua(), (table.clone(),))?;
            let back = StructuredLiveDocument::decode(&Value::Table(table), self.lua())
                .map_err(EngineError::from)?;
            check_live_path(destination, &back.path)?;
            if back.body.format != *format {
                return Err(EngineError::Structured {
                    display: display.to_owned(),
                    base: format.name().to_owned(),
                    patch: back.body.format.name().to_owned(),
                });
            }
            data = merge(display, data, &mut owners, back.body.data, &patch.owner);
        }
        Ok(data)
    }

    /// Run patch callbacks over rc sections through live documents keeping
    /// first entries.
    ///
    /// - Callbacks run over live documents.
    /// - Callbacks keep the path fixed.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Mismatch`] for tables outside the live shape.
    /// - [`EngineError::Lua`] for bridge, callback, and path drift failures.
    pub(super) fn run_rc_callbacks(
        &self,
        destination: &Route,
        display: &str,
        document: RcData,
        patches: &[&OwnedPatch],
    ) -> Result<RcData> {
        let mut document = document;
        let mut owned = seed_collision_owners(&document);
        for patch in patches {
            let Some(found) = patch.rc() else {
                continue;
            };
            let live = RcLiveDocument {
                path: destination.clone(),
                body: document.clone(),
            };
            let table = live_view(self.lua(), &live)?;
            found.apply.call::<()>(self.lua(), (table.clone(),))?;
            let back = RcLiveDocument::decode(&Value::Table(table), self.lua())
                .map_err(EngineError::from)?;
            check_live_path(destination, &back.path)?;
            document = merge_rc(display, &document, back.body, &mut owned, &patch.owner);
        }
        Ok(document)
    }
}

/// Build one live view from its face for patch callbacks.
///
/// # Errors
///
/// - [`EngineError::Lua`] for faces outside the table shape.
fn live_view(lua: &Lua, document: &impl Encode) -> Result<Table> {
    match document.encode(lua)? {
        Value::Table(table) => Ok(table),
        _ => Err(EngineError::Lua(mlua::Error::runtime(
            "patch live views hold tables",
        ))),
    }
}

/// Fail when one patch callback moves its live document off its target.
///
/// # Errors
///
/// - [`EngineError::Lua`] for paths apart from the target.
fn check_live_path(destination: &Route, back: &Route) -> Result<()> {
    if back != destination {
        return Err(EngineError::Lua(mlua::Error::runtime(format!(
            "patch target '{}' changed path to '{}'; patches keep the path fixed",
            destination.display(),
            back.display()
        ))));
    }
    Ok(())
}

/// Seed key owners from base data under the base name.
fn seed_key_owners(data: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, String> {
    let mut owners = BTreeMap::new();
    for (key, value) in data {
        seed_leaves(value, key, "base", &mut owners);
    }
    owners
}

/// Seed leaf owners under one path with one holder.
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

/// Merge patch data over live data keeping first writes.
fn merge(
    display: &str,
    live: BTreeMap<String, serde_json::Value>,
    owners: &mut BTreeMap<String, String>,
    next: BTreeMap<String, serde_json::Value>,
    owner: &str,
) -> BTreeMap<String, serde_json::Value> {
    let loser = owner;
    let mut merged = live;
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

/// Fold one top-level entry into merged data with leaf owners.
fn fold_entry(
    display: &str,
    merged: &mut BTreeMap<String, serde_json::Value>,
    owners: &mut BTreeMap<String, String>,
    key: String,
    next_value: serde_json::Value,
    loser: &str,
) {
    let Some(live) = merged.get_mut(&key) else {
        seed_leaves(&next_value, &key, loser, owners);
        merged.insert(key, next_value);
        return;
    };
    if *live == next_value {
        return;
    }
    fold_value(display, &key, live, next_value, owners, loser);
}

/// Fold one patch value over one live value keeping first writes.
fn fold_value(
    display: &str,
    path: &str,
    live: &mut serde_json::Value,
    next: serde_json::Value,
    owners: &mut BTreeMap<String, String>,
    loser: &str,
) {
    if *live == next {
        return;
    }
    let live_empty = live.as_object().map(|map| map.is_empty()).unwrap_or(false);
    match (live, next) {
        (serde_json::Value::Object(live_map), serde_json::Value::Object(next_map)) => {
            fold_maps(display, path, live_map, next_map, owners, loser);
        }
        (live, found) => {
            if live_empty {
                seed_leaves(&found, path, loser, owners);
                *live = found;
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

/// Fold one patch table over one live table keeping first writes.
fn fold_maps(
    display: &str,
    path: &str,
    live_map: &mut serde_json::Map<String, serde_json::Value>,
    next_map: serde_json::Map<String, serde_json::Value>,
    owners: &mut BTreeMap<String, String>,
    loser: &str,
) {
    let removed: Vec<String> = live_map
        .keys()
        .filter(|key| !next_map.contains_key(*key))
        .cloned()
        .collect();
    for (key, next_value) in next_map {
        let child = format!("{path}.{key}");
        if let Some(live) = live_map.get_mut(&key) {
            if *live == next_value {
                continue;
            }
            fold_value(display, &child, live, next_value, owners, loser);
        } else {
            seed_leaves(&next_value, &child, loser, owners);
            live_map.insert(key, next_value);
        }
    }
    for key in removed {
        if let Some(found) = live_map.get(&key).cloned() {
            let child = format!("{path}.{key}");
            log_removed(display, &found, &child, owners, loser);
        }
    }
}

/// Log owned leaves under one removed path with the losing owner.
fn log_removed(
    display: &str,
    live: &serde_json::Value,
    path: &str,
    owners: &BTreeMap<String, String>,
    loser: &str,
) {
    match live {
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

/// Return the winning owner for one collision path.
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

/// Winner owner per rc collision feeding first-writer wins.
#[derive(Default)]
struct CollisionOwners {
    /// Collision owners in the profile section.
    profile: BTreeMap<String, String>,
    /// Collision owners in the config section.
    config: BTreeMap<String, String>,
    /// Collision owners in the final section.
    final_entries: BTreeMap<String, String>,
}

/// Seed collision owners from base entries under the base name.
fn seed_collision_owners(document: &RcData) -> CollisionOwners {
    let mut owned = CollisionOwners::default();
    seed_section(&document.profile, &mut owned.profile);
    seed_section(&document.config, &mut owned.config);
    seed_section(&document.r#final, &mut owned.final_entries);
    owned
}

/// Seed one section map from its entries under the base name.
fn seed_section(entries: &[RcEntry], owned: &mut BTreeMap<String, String>) {
    for entry in entries {
        if let Some(name) = entry.collision_key() {
            owned.insert(name.to_owned(), "base".to_owned());
        }
    }
}

/// Merge patch sections over live sections keeping first entries.
fn merge_rc(
    display: &str,
    live: &RcData,
    next: RcData,
    owned: &mut CollisionOwners,
    owner: &str,
) -> RcData {
    let loser = owner;
    RcData {
        profile: merge_section(
            display,
            "profile",
            &live.profile,
            next.profile,
            &mut owned.profile,
            loser,
        ),
        config: merge_section(
            display,
            "config",
            &live.config,
            next.config,
            &mut owned.config,
            loser,
        ),
        r#final: merge_section(
            display,
            "final",
            &live.r#final,
            next.r#final,
            &mut owned.final_entries,
            loser,
        ),
    }
}

/// Merge one patch section over its live entries keeping first entries.
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
        let live: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "latest"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([(
            "tools".to_owned(),
            json!({"bat": "latest", "ripgrep": "latest"}),
        )]);
        let merged = merge("home('config.json')", live, &mut owners, next, "second");
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
        let live: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "first"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("tools".to_owned(), json!({"bat": "second"}))]);
        let merged = merge("home('config.json')", live, &mut owners, next, "second");
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("first")
        );
        assert_eq!(owners.get("tools.bat").map(String::as_str), Some("first"));
    }

    #[test]
    fn table_against_scalar_keeps_first_write() {
        let live: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("tools".to_owned(), json!({"bat": "kept"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("tools".to_owned(), json!("flat"))]);
        let merged = merge("home('config.json')", live, &mut owners, next, "second");
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("kept")
        );
    }

    #[test]
    fn lists_stay_whole_value_first_wins() {
        let live: BTreeMap<String, serde_json::Value> =
            BTreeMap::from([("items".to_owned(), json!([1, 2]))]);
        let mut owners = BTreeMap::from([("items".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("items".to_owned(), json!([3]))]);
        let merged = merge("home('config.json')", live, &mut owners, next, "second");
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
