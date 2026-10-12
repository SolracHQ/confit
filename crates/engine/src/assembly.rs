//! Assembly
//! Assemble validated profiles into finished documents.

mod collect;
mod freeze;
mod hooks;
mod merge;
pub(crate) mod owned;
mod patches;

use std::collections::BTreeMap;

use mlua::Lua;

use crate::api::profile::Profile;
use confit_model::document::Data;
use confit_model::hook::Hook;
use confit_model::routes::Route;

use crate::error::Result;

/// Validated profile assembly into finished documents.
pub struct Assembler<'a> {
    lua: &'a Lua,
}

impl<'a> Assembler<'a> {
    /// Build one assembler over live Lua.
    pub fn new(lua: &'a Lua) -> Self {
        Self { lua }
    }

    /// Return the live Lua instance.
    pub(super) fn lua(&self) -> &Lua {
        self.lua
    }

    /// Collect one profile into payloads keyed by destination route.
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
    /// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
    pub fn assemble(&self, profile: &Profile) -> Result<BTreeMap<Route, Data>> {
        collect::check_names(profile)?;
        let placed = collect::dedupe(profile)?;
        let owned = patches::split_patches(profile);
        let mut finished = BTreeMap::new();
        self.apply_structured(
            &placed,
            &patches::grouped_structured(self.lua, &placed, &owned)?,
            &mut finished,
        )?;
        self.apply_rc(
            &placed,
            &patches::grouped_rc(self.lua, &placed, &owned)?,
            &mut finished,
        )?;
        for (destination, body) in placed {
            if finished.contains_key(&destination) {
                continue;
            }
            finished.insert(destination, body);
        }
        freeze::check_rc_guards(&finished)?;
        Ok(finished)
    }

    /// Collect one profile's hooks in first-seen order.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::ChangedUnknown`] for conditions with
    ///   unbuilt routes.
    pub fn collect_hooks(
        &self,
        profile: &Profile,
        documents: &BTreeMap<Route, Data>,
    ) -> Result<Vec<Hook>> {
        hooks::collect_hooks(profile, documents)
    }
}

/// Collect one profile into payloads keyed by destination route.
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
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub fn assemble(lua: &Lua, profile: &Profile) -> Result<BTreeMap<Route, Data>> {
    Assembler::new(lua).assemble(profile)
}
