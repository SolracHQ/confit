//! Assembly
//! Assemble validated profiles into finished documents.

mod collect;
mod freeze;
mod hooks;
mod merge;
mod patches;

use std::collections::BTreeMap;

use mlua::Lua;

use confit_model::document::Document as ModelDocument;
use confit_model::hook::Hook;
use confit_store::Stores;
use confit_surface_api::Profile;

use crate::error::Result;

/// Validated profile assembly into finished documents.
pub struct Assembler<'a> {
    lua: &'a Lua,
    stores: &'a Stores,
}

impl<'a> Assembler<'a> {
    /// Build one assembler over live Lua with shared stores.
    pub fn new(lua: &'a Lua, stores: &'a Stores) -> Self {
        Self { lua, stores }
    }

    /// Return the live Lua instance.
    pub(super) fn lua(&self) -> &Lua {
        self.lua
    }

    /// Return the shared stores.
    pub(super) fn stores(&self) -> &Stores {
        self.stores
    }

    /// Collect one profile into documents in stable route order.
    ///
    /// - Patches merge in rank-descending level order over structured maps
    ///   and rc sections with first writes winning.
    /// - Filters fan out over matched destinations in destination order.
    /// - Callbacks run with the target route beside live data.
    /// - Unknown targets fail the plan.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
    /// - [`crate::error::EngineError::Defined`] for repeated config names.
    /// - [`crate::error::EngineError::UnknownTarget`] for missing targets.
    /// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
    /// - [`crate::error::EngineError::ChangedInRc`] for rc guards with
    ///   hooks-only changed leaves.
    /// - [`crate::error::EngineError::Model`] for documents failing hashes.
    /// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
    pub fn assemble(&self, profile: &Profile) -> Result<Vec<ModelDocument>> {
        collect::check_names(profile)?;
        let placed = collect::dedupe(profile)?;
        let (structured, rc) = patches::split_patches(profile);
        let mut finished = BTreeMap::new();
        self.apply_structured(
            &placed,
            &patches::grouped_structured(self.lua, &placed, &structured)?,
            &mut finished,
        )?;
        self.apply_rc(
            &placed,
            &patches::grouped_rc(self.lua, &placed, &rc)?,
            &mut finished,
        )?;
        for (destination, body) in placed {
            if finished.contains_key(&destination) {
                continue;
            }
            finished.insert(
                destination.clone(),
                self.placed_document(&destination, &body)?,
            );
        }
        freeze::check_rc_guards(&finished)?;
        freeze::fill_hashes(&mut finished)?;
        Ok(finished.into_values().collect())
    }

    /// Collect one profile's hooks in first-seen order.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::ChangedUnknown`] for gates with
    ///   unbuilt routes.
    pub fn collect_hooks(
        &self,
        profile: &Profile,
        documents: &[ModelDocument],
    ) -> Result<Vec<Hook>> {
        hooks::collect_hooks(profile, documents)
    }
}

/// Collect one profile into documents in stable route order.
///
/// - Patches merge in rank-descending level order over structured maps and rc
///   sections with first writes winning.
/// - Filters fan out over matched destinations in destination order.
/// - Callbacks run with the target route beside live data.
/// - Unknown targets fail the plan.
///
/// # Errors
///
/// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
/// - [`crate::error::EngineError::Defined`] for repeated config names.
/// - [`crate::error::EngineError::UnknownTarget`] for missing targets.
/// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
/// - [`crate::error::EngineError::ChangedInRc`] for rc guards with
///   hooks-only changed leaves.
/// - [`crate::error::EngineError::Model`] for documents failing hashes.
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub fn assemble(lua: &Lua, stores: &Stores, profile: &Profile) -> Result<Vec<ModelDocument>> {
    Assembler::new(lua, stores).assemble(profile)
}

/// Collect one profile's hooks in first-seen order.
///
/// # Errors
///
/// - [`crate::error::EngineError::ChangedUnknown`] for gates showing unbuilt
///   routes.
pub fn collect_hooks(profile: &Profile, documents: &[ModelDocument]) -> Result<Vec<Hook>> {
    hooks::collect_hooks(profile, documents)
}
