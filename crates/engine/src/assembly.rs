//! Assembly
//! Assemble validated profiles into finished documents.

mod collect;
mod freeze;
mod hooks;
mod merge;
pub(crate) mod owned;
mod patches;

use mlua::Lua;

use crate::api::profile::Profile;
use confit_model::document::Document;
use confit_model::hook::Hook;

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

    /// Collect one profile into documents in destination order.
    ///
    /// - Patches merge in rank-descending level order over structured maps
    ///   and rc sections with first writes winning.
    /// - Groups follow first-seen declaration order.
    /// - Filters fan out over matched destinations in destination order.
    /// - Callbacks run over live documents.
    /// - Unknown targets fail the plan.
    /// - Unknown requirement names fail the plan with the hint.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
    /// - [`crate::error::EngineError::Defined`] for repeated config names.
    /// - [`crate::error::EngineError::Require`] for requirements showing unknown names.
    /// - [`crate::error::EngineError::UnknownTarget`] for missing targets.
    /// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
    /// - [`crate::error::EngineError::ChangedInRc`] for rc guards with
    ///   hooks-only changed leaves.
    /// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
    pub fn assemble(&self, profile: &Profile) -> Result<Vec<Document>> {
        collect::check_names(profile)?;
        collect::check_requirements(profile)?;
        let placed = collect::dedupe(profile)?;
        let owned = patches::split_patches(profile);
        let mut finished: Vec<Document> = Vec::new();
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
        for document in &placed {
            if finished.iter().any(|found| found.path == document.path) {
                continue;
            }
            finished.push(document.clone());
        }
        finished.sort_by(|left, right| left.path.cmp(&right.path));
        freeze::check_rc_guards(&finished)?;
        Ok(finished)
    }

    /// Collect one profile's hooks in first-seen order.
    ///
    /// - Hook patches mutate live hooks in return order.
    /// - Unknown hook targets and repeated ids fail the plan.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::DuplicateHook`] for repeated hook ids.
    /// - [`crate::error::EngineError::UnknownHookTarget`] for patches with
    ///   unknown ids.
    /// - [`crate::error::EngineError::ChangedUnknown`] for conditions with
    ///   unbuilt routes.
    /// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
    pub fn collect_hooks(&self, profile: &Profile, documents: &[Document]) -> Result<Vec<Hook>> {
        hooks::collect_hooks(self.lua, profile, documents)
    }
}

/// Collect one profile into documents in destination order.
///
/// - Patches merge in rank-descending level order over structured maps and rc
///   sections with first writes winning.
/// - Groups follow first-seen declaration order.
/// - Filters fan out over matched destinations in destination order.
/// - Callbacks run over live documents.
/// - Unknown targets fail the plan.
/// - Unknown requirement names fail the plan with the hint.
///
/// # Errors
///
/// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
/// - [`crate::error::EngineError::Defined`] for repeated config names.
/// - [`crate::error::EngineError::Require`] for requirements showing unknown names.
/// - [`crate::error::EngineError::UnknownTarget`] for missing targets.
/// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
/// - [`crate::error::EngineError::ChangedInRc`] for rc guards with
///   hooks-only changed leaves.
/// - [`crate::error::EngineError::Lua`] for bridge and callback failures.
pub fn assemble(lua: &Lua, profile: &Profile) -> Result<Vec<Document>> {
    Assembler::new(lua).assemble(profile)
}
