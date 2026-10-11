//! Merge patch callbacks under first-writer wins.

use std::collections::BTreeMap;

use mlua::{FromLua, LuaSerdeExt, Value};

use confit_model::document::{
    Data, Document as ModelDocument, RcData, RcEntry, StructuredFormat, Table,
};
use confit_model::routes::Route;
use confit_surface_api::{
    Document, RcDocument, RcPatch, SerializableValue, StructuredDocument, StructuredPatch,
    route_value,
};

use super::Assembler;
use super::patches::{rc_base_of, structured_base_of};
use crate::error::{EngineError, Result};

impl Assembler<'_> {
    /// Apply structured patch groups over declared bases into documents.
    ///
    /// - First writes win in rank-descending level order with collision logs.
    /// - Callbacks run with the target route beside live data.
    ///
    /// # Errors
    ///
    /// - [`EngineError::UnknownTarget`] for missing targets.
    /// - [`EngineError::Structured`] for foreign bases with gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn apply_structured(
        &self,
        placed: &BTreeMap<Route, Document>,
        grouped: &BTreeMap<Route, Vec<&StructuredPatch>>,
        finished: &mut BTreeMap<Route, ModelDocument>,
    ) -> Result<()> {
        for (target, patches) in grouped {
            let Some(first) = patches.first() else {
                continue;
            };
            let display = target.display();
            let (destination, format, data) = structured_base_of(placed, target, first, patches)?;
            let data = self.run_callbacks(target, &display, &format, data, patches)?;
            let document = ModelDocument::new(destination, Data::Structured { format, data });
            finished.insert(target.clone(), document);
        }
        Ok(())
    }

    /// Apply rc patch groups over declared bases into documents.
    ///
    /// - First writes win in rank-descending level order with collision logs.
    /// - Callbacks run with the target route beside live sections.
    ///
    /// # Errors
    ///
    /// - [`EngineError::UnknownTarget`] for missing targets.
    /// - [`EngineError::Structured`] for foreign bases with gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn apply_rc(
        &self,
        placed: &BTreeMap<Route, Document>,
        grouped: &BTreeMap<Route, Vec<&RcPatch>>,
        finished: &mut BTreeMap<Route, ModelDocument>,
    ) -> Result<()> {
        for (target, patches) in grouped {
            let (destination, document) = rc_base_of(placed, target)?;
            let display = target.display();
            let patched = self.run_rc_callbacks(target, &display, document, patches)?;
            let data = RcData::new(patched.profile, patched.config, patched.final_entries);
            let document = ModelDocument::new(destination, Data::Rc(data));
            finished.insert(target.clone(), document);
        }
        Ok(())
    }

    /// Run patch callbacks over base data through live documents keeping
    /// first writes.
    ///
    /// - Callbacks run with the target route beside live data.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Structured`] for format gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn run_callbacks(
        &self,
        destination: &Route,
        display: &str,
        format: &StructuredFormat,
        data: Table,
        patches: &[&StructuredPatch],
    ) -> Result<Table> {
        let mut data = data;
        let mut owners = seed_key_owners(&data);
        for patch in patches {
            let body = StructuredDocument {
                format: *format,
                data: data
                    .iter()
                    .map(|(key, value)| (key.clone(), SerializableValue(value.clone())))
                    .collect(),
            };
            let route = route_value(self.lua(), destination)?;
            let table: Value = self.lua().to_value(&body)?;
            patch
                .apply
                .as_function()
                .call::<()>((route, table.clone()))?;
            let found = StructuredDocument::from_lua(table, self.lua())?;
            if found.format != *format {
                return Err(EngineError::Structured {
                    display: display.to_owned(),
                    base: format.name().to_owned(),
                    patch: found.format.name().to_owned(),
                });
            }
            data = merge(
                display,
                data,
                &mut owners,
                found.data,
                patch.owner.as_deref(),
            );
        }
        Ok(data)
    }

    /// Run patch callbacks over rc sections through live tables keeping
    /// first entries.
    ///
    /// - Callbacks run with the target route beside live sections.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn run_rc_callbacks(
        &self,
        destination: &Route,
        display: &str,
        document: RcDocument,
        patches: &[&RcPatch],
    ) -> Result<RcDocument> {
        let mut document = document;
        let mut owned = seed_slot_owners(&document);
        for patch in patches {
            let route = route_value(self.lua(), destination)?;
            let table: Value = Value::Table(document.live_table(self.lua())?);
            patch
                .apply
                .as_function()
                .call::<()>((route, table.clone()))?;
            let next = RcDocument::from_lua(table, self.lua())?;
            document = merge_rc(display, &document, next, &mut owned, patch.owner.as_deref());
        }
        Ok(document)
    }
}

/// Seed key owners from base data under the base name.
fn seed_key_owners(data: &Table) -> BTreeMap<String, String> {
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
    live: Table,
    owners: &mut BTreeMap<String, String>,
    next: BTreeMap<String, SerializableValue>,
    owner: Option<&str>,
) -> Table {
    let loser = owner.unwrap_or("unknown");
    let mut merged = live;
    let missing: Vec<String> = merged
        .keys()
        .filter(|key| !next.contains_key(*key))
        .cloned()
        .collect();
    for (key, value) in next {
        fold_entry(display, &mut merged, owners, key, value.0, loser);
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
    merged: &mut Table,
    owners: &mut BTreeMap<String, String>,
    key: String,
    next_value: serde_json::Value,
    loser: &str,
) {
    let Some(slot) = merged.get_mut(&key) else {
        seed_leaves(&next_value, &key, loser, owners);
        merged.insert(key, next_value);
        return;
    };
    if *slot == next_value {
        return;
    }
    fold_value(display, &key, slot, next_value, owners, loser);
}

/// Fold one patch value over one live slot keeping first writes.
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
        (slot, found) => {
            if live_empty {
                seed_leaves(&found, path, loser, owners);
                *slot = found;
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
        if let Some(slot) = live_map.get_mut(&key) {
            if *slot == next_value {
                continue;
            }
            fold_value(display, &child, slot, next_value, owners, loser);
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

/// Winner owner per rc slot feeding first-writer wins.
#[derive(Default)]
struct SlotOwners {
    /// Slot owners in the profile section.
    profile: BTreeMap<String, String>,
    /// Slot owners in the config section.
    config: BTreeMap<String, String>,
    /// Slot owners in the final section.
    final_entries: BTreeMap<String, String>,
}

/// Seed slot owners from base entries with slots under the base name.
fn seed_slot_owners(document: &RcDocument) -> SlotOwners {
    let mut owned = SlotOwners::default();
    seed_section(&document.profile, &mut owned.profile);
    seed_section(&document.config, &mut owned.config);
    seed_section(&document.final_entries, &mut owned.final_entries);
    owned
}

/// Seed one section map from its entries with slots under the base name.
fn seed_section(entries: &[RcEntry], owned: &mut BTreeMap<String, String>) {
    for entry in entries {
        if let Some(name) = entry.slot_name() {
            owned.insert(name.to_owned(), "base".to_owned());
        }
    }
}

/// Merge patch sections over live sections keeping first entries.
fn merge_rc(
    display: &str,
    live: &RcDocument,
    next: RcDocument,
    owned: &mut SlotOwners,
    owner: Option<&str>,
) -> RcDocument {
    let loser = owner.unwrap_or("unknown");
    RcDocument {
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
        final_entries: merge_section(
            display,
            "final",
            &live.final_entries,
            next.final_entries,
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
        let slot = entry.slot_name().map(str::to_owned);
        match slot {
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
        let live: Table = BTreeMap::from([("tools".to_owned(), json!({"bat": "latest"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([(
            "tools".to_owned(),
            SerializableValue(json!({"bat": "latest", "ripgrep": "latest"})),
        )]);
        let merged = merge(
            "home('config.json')",
            live,
            &mut owners,
            next,
            Some("second"),
        );
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
        let live: Table = BTreeMap::from([("tools".to_owned(), json!({"bat": "first"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([(
            "tools".to_owned(),
            SerializableValue(json!({"bat": "second"})),
        )]);
        let merged = merge(
            "home('config.json')",
            live,
            &mut owners,
            next,
            Some("second"),
        );
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("first")
        );
        assert_eq!(owners.get("tools.bat").map(String::as_str), Some("first"));
    }

    #[test]
    fn table_against_scalar_keeps_first_write() {
        let live: Table = BTreeMap::from([("tools".to_owned(), json!({"bat": "kept"}))]);
        let mut owners = BTreeMap::from([("tools.bat".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("tools".to_owned(), SerializableValue(json!("flat")))]);
        let merged = merge(
            "home('config.json')",
            live,
            &mut owners,
            next,
            Some("second"),
        );
        let tools = merged.get("tools").unwrap();
        assert_eq!(
            tools.get("bat").and_then(|item| item.as_str()),
            Some("kept")
        );
    }

    #[test]
    fn lists_stay_whole_value_first_wins() {
        let live: Table = BTreeMap::from([("items".to_owned(), json!([1, 2]))]);
        let mut owners = BTreeMap::from([("items".to_owned(), "first".to_owned())]);
        let next = BTreeMap::from([("items".to_owned(), SerializableValue(json!([3])))]);
        let merged = merge(
            "home('config.json')",
            live,
            &mut owners,
            next,
            Some("second"),
        );
        assert_eq!(merged.get("items"), Some(&json!([1, 2])));
        assert_eq!(owners.get("items").map(String::as_str), Some("first"));
    }

    #[test]
    fn seeded_base_leaves_beat_patch_writes() {
        let base: Table = BTreeMap::from([("tools".to_owned(), json!({"bat": "base"}))]);
        let mut owners = seed_key_owners(&base);
        let next = BTreeMap::from([(
            "tools".to_owned(),
            SerializableValue(json!({"bat": "patch", "ripgrep": "latest"})),
        )]);
        let merged = merge(
            "home('config.json')",
            base,
            &mut owners,
            next,
            Some("patch"),
        );
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
