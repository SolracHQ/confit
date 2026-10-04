//! Merged patch callbacks.
//! Structured maps with rc sections under first-writer wins.

use std::collections::BTreeMap;

use mlua::{FromLua, LuaSerdeExt, Value};

use confit_model::document::{
    Data, Document as ModelDocument, RcData, RcEntry, StructuredFormat, Table,
};
use confit_model::routes::Route;
use confit_surface_api::{
    Document, RcDocument, RcPatch, SerializableValue, StructuredDocument, StructuredPatch,
};

use super::Assembler;
use super::patches::{rc_base_of, structured_base_of};
use crate::error::{EngineError, Result};

impl Assembler<'_> {
    /// Applies structured patch groups over declared bases into documents.
    ///
    /// - First writes win in rank-descending level order with collision logs.
    ///
    /// # Errors
    ///
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
            let data = self.run_callbacks(&display, &format, data, patches)?;
            let document = ModelDocument::new(destination, Data::Structured { format, data });
            finished.insert(target.clone(), document);
        }
        Ok(())
    }

    /// Applies rc patch groups over declared bases into documents.
    ///
    /// - First writes win in rank-descending level order with collision logs.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Structured`] for foreign bases with gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn apply_rc(
        &self,
        placed: &BTreeMap<Route, Document>,
        grouped: &BTreeMap<Route, Vec<&RcPatch>>,
        finished: &mut BTreeMap<Route, ModelDocument>,
    ) -> Result<()> {
        for (target, patches) in grouped {
            let Some(first) = patches.first() else {
                continue;
            };
            let (destination, document) = rc_base_of(placed, target, first)?;
            let display = target.display();
            let patched = self.run_rc_callbacks(&display, document, patches)?;
            let data = RcData::new(patched.profile, patched.config, patched.final_entries);
            let document = ModelDocument::new(destination, Data::Rc(data));
            finished.insert(target.clone(), document);
        }
        Ok(())
    }

    /// Runs patch callbacks over base data through live documents.
    ///
    /// - Owned keys keep their values against later writes.
    /// - Fresh keys ride the writing patch owner.
    /// - Blocked writes log naming target with key and losing owner.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Structured`] for format gaps.
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn run_callbacks(
        &self,
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
            let table: Value = self.lua().to_value(&body)?;
            patch.apply().call::<()>(table.clone())?;
            let found = StructuredDocument::from_lua(table, self.lua())?;
            if found.format != *format {
                return Err(EngineError::Structured {
                    display: display.to_owned(),
                    base: format.name().to_owned(),
                    patch: found.format.name().to_owned(),
                });
            }
            data = merge_first_wins(display, data, &mut owners, found.data, patch.owner());
        }
        Ok(data)
    }

    /// Runs patch callbacks over rc sections through live tables.
    ///
    /// - Owned slots keep their first entry against later duplicates.
    /// - Fresh slots ride the writing patch owner.
    /// - Dropped duplicates log naming target with section, slot and losing owner.
    ///
    /// # Errors
    ///
    /// - [`EngineError::Lua`] for bridge and callback failures.
    pub(super) fn run_rc_callbacks(
        &self,
        display: &str,
        document: RcDocument,
        patches: &[&RcPatch],
    ) -> Result<RcDocument> {
        let mut document = document;
        let mut owned = seed_slot_owners(&document);
        for patch in patches {
            let table: Value = Value::Table(document.live_table(self.lua())?);
            patch.apply().call::<()>(table.clone())?;
            let next = RcDocument::from_lua(table, self.lua())?;
            document = merge_rc_first_wins(display, &document, next, &mut owned, patch.owner());
        }
        Ok(document)
    }
}

/// Seeds key owners from base data under the base name.
///
/// - Every base key reads owned by the base.
fn seed_key_owners(data: &Table) -> BTreeMap<String, String> {
    data.keys()
        .map(|key| (key.clone(), "base".to_owned()))
        .collect()
}

/// Merges patch data over live data keeping first writes.
///
/// - Unchanged keys ride through silently.
/// - Owned keys keep their values against later writes.
/// - Fresh keys ride the writing patch owner.
/// - Blocked writes log naming target with key and losing owner.
fn merge_first_wins(
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
        if merged.get(&key) == Some(&value.0) {
            continue;
        }
        match owners.get(&key) {
            Some(winner) => {
                log::warn!(
                    "collision on {display} \"{key}\": \"{loser}\" loses, \"{winner}\" wins"
                );
            }
            None => {
                owners.insert(key.clone(), loser.to_owned());
                merged.insert(key, value.0);
            }
        }
    }
    for key in missing {
        if let Some(winner) = owners.get(&key) {
            log::warn!("collision on {display} \"{key}\": \"{loser}\" loses, \"{winner}\" wins");
        }
    }
    merged
}

/// Winner owner per rc slot feeding first-writer wins.
///
/// - Base entries seed every map under the base name.
/// - Later duplicates drop with a log line.
#[derive(Default)]
struct SlotOwners {
    /// Slot owners in the profile section.
    profile: BTreeMap<String, String>,
    /// Slot owners in the config section.
    config: BTreeMap<String, String>,
    /// Slot owners in the final section.
    final_entries: BTreeMap<String, String>,
}

/// Seeds slot owners from base entries under the base name.
///
/// - Slot-free entries skip the sets.
fn seed_slot_owners(document: &RcDocument) -> SlotOwners {
    let mut owned = SlotOwners::default();
    seed_section(&document.profile, &mut owned.profile);
    seed_section(&document.config, &mut owned.config);
    seed_section(&document.final_entries, &mut owned.final_entries);
    owned
}

/// Seeds one section map from its entries under the base name.
///
/// - Slot-free entries skip the map.
fn seed_section(entries: &[RcEntry], owned: &mut BTreeMap<String, String>) {
    for entry in entries {
        if let Some(name) = entry.slot_name() {
            owned.insert(name.to_owned(), "base".to_owned());
        }
    }
}

/// Merges patch sections over live sections keeping first entries.
///
/// - Carried entries ride through silently.
/// - Owned slots keep their first entry against later duplicates.
/// - Fresh slots ride the writing patch owner.
/// - Dropped duplicates log naming target with section, slot and losing owner.
fn merge_rc_first_wins(
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

/// Merges one patch section over its live entries keeping first entries.
///
/// - Carried entries ride through silently.
/// - Owned slots keep their first entry against later duplicates.
/// - Fresh slots ride the writing patch owner.
/// - Dropped duplicates log naming target with section, slot and losing owner.
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
