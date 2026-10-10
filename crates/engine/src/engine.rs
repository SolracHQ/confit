//! Engine
//! Evaluate Lua profiles into finished manifests.

use std::path::Path;

use mlua::Lua;

use confit_model::manifest::Manifest;
use confit_store::Stores;

use crate::error::Result;

/// Profile evaluator.
pub struct Engine {
    /// The Lua instance.
    lua: Lua,
    /// Shared store access.
    stores: Stores,
}

impl Engine {
    /// Build one engine from stores.
    pub fn new(stores: Stores) -> Self {
        Self {
            lua: Lua::new(),
            stores,
        }
    }

    /// Evaluate one profile file into a finished manifest.
    ///
    /// - Documents finish with filled hashes in destination display order.
    /// - Patches merge in rank-descending level order over structured maps
    ///   and rc sections with first writes winning.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::Surface`] for surface failures.
    /// - [`crate::error::EngineError::Model`] for unhashable documents.
    /// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
    /// - [`crate::error::EngineError::Defined`] for repeated config names.
    /// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
    /// - [`crate::error::EngineError::ChangedInRc`] for rc guards with
    ///   hooks-only changed leaves.
    /// - [`crate::error::EngineError::Read`] for unreadable files.
    /// - [`crate::error::EngineError::Utf8`] for non-UTF8 files.
    /// - [`crate::error::EngineError::Lua`] for Lua failures.
    pub fn evaluate(&self, profile: &Path) -> Result<Manifest> {
        self.lua.set_app_data(self.stores.clone());
        crate::loader::install(&self.lua)?;
        let value = crate::loader::load_chunk(&self.lua, profile)?;
        let decoded = confit_surface_api::Profile::decode(&self.lua, value)?;
        let documents = crate::assembly::assemble(&self.lua, &self.stores, &decoded)?;
        let hooks = crate::assembly::collect_hooks(&decoded, &documents)?;
        Ok(Manifest::build(documents, hooks)?)
    }
}
