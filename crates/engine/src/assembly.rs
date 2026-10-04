//! Assembly
//! Validated profiles into finished documents.

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

/// Assembles validated profiles into finished documents.
///
/// - Lua callbacks run through the shared instance with first writes winning.
/// - Blob handles open through shared stores with hashes filled last.
pub struct Assembler<'a> {
    lua: &'a Lua,
    stores: &'a Stores,
}

impl<'a> Assembler<'a> {
    /// Builds one assembler over live Lua with shared stores.
    pub fn new(lua: &'a Lua, stores: &'a Stores) -> Self {
        Self { lua, stores }
    }

    /// Reads the live Lua instance.
    pub(super) fn lua(&self) -> &Lua {
        self.lua
    }

    /// Reads the shared stores.
    pub(super) fn stores(&self) -> &Stores {
        self.stores
    }

    /// Collects one profile into documents in stable route order.
    ///
    /// - Patches merge in rank-descending level order over structured maps
    ///   and rc sections with first writes winning.
    /// - Missing targets seed empty maps and empty sections, foreign bases
    ///   fail with format gaps.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
    /// - [`crate::error::EngineError::Defined`] for repeated config names.
    /// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
    /// - [`crate::error::EngineError::ChangedInRc`] for rc guards holding
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
            &patches::grouped_structured(&structured),
            &mut finished,
        )?;
        self.apply_rc(&placed, &patches::grouped_rc(&rc), &mut finished)?;
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

    /// Collects one profile's hooks in first-seen order.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::ChangedUnknown`] for gates naming
    ///   unbuilt routes.
    pub fn collect_hooks(
        &self,
        profile: &Profile,
        documents: &[ModelDocument],
    ) -> Result<Vec<Hook>> {
        hooks::collect_hooks(profile, documents)
    }
}

/// Collects one profile into documents in stable route order.
///
/// - Patches merge in rank-descending level order over structured maps and rc
///   sections with first writes winning.
/// - Missing targets seed empty maps and empty sections, foreign bases fail
///   with format gaps.
///
/// # Errors
///
/// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
/// - [`crate::error::EngineError::Defined`] for repeated config names.
/// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
/// - [`crate::error::EngineError::ChangedInRc`] for rc guards holding
///   hooks-only changed leaves.
/// - [`crate::error::EngineError::Model`] for documents failing hashes.
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub fn assemble(lua: &Lua, stores: &Stores, profile: &Profile) -> Result<Vec<ModelDocument>> {
    Assembler::new(lua, stores).assemble(profile)
}

/// Collects one profile's hooks in first-seen order.
///
/// # Errors
///
/// - [`crate::error::EngineError::ChangedUnknown`] for gates naming unbuilt
///   routes.
pub fn collect_hooks(profile: &Profile, documents: &[ModelDocument]) -> Result<Vec<Hook>> {
    hooks::collect_hooks(profile, documents)
}
