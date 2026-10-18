//! Engine
//! Evaluate Lua profiles into documents with hooks.

use std::path::Path;

use mlua::Lua;

use confit_model::document::Document;
use confit_model::hook::Hook;
use confit_store::Stores;

use confit_binding::value::Decode;

use crate::api::profile::Profile;
use crate::assembly::Assembler;
use crate::error::Result;
use crate::loader::{install, load_chunk};
use crate::project::Project;

/// Evaluated profile documents with hooks.
pub struct Evaluated {
    /// Assembled documents in destination order.
    pub documents: Vec<Document>,
    /// Collected hooks in first-seen order.
    pub hooks: Vec<Hook>,
}

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

    /// Evaluate one profile file into documents with hooks.
    ///
    /// - Documents arrive assembled in destination order.
    /// - Patches merge in rank-descending level order over structured maps
    ///   and rc sections with first writes winning.
    /// - Filters fan out over matched destinations in destination order.
    /// - Unknown targets fail the plan.
    ///
    /// # Errors
    ///
    /// - [`crate::error::EngineError::Mismatch`] for misshaped profiles.
    /// - [`crate::error::EngineError::Duplicate`] for repeated destinations.
    /// - [`crate::error::EngineError::Defined`] for repeated config names.
    /// - [`crate::error::EngineError::Require`] for requirements showing unknown names.
    /// - [`crate::error::EngineError::UnknownTarget`] for missing targets.
    /// - [`crate::error::EngineError::DuplicateHook`] for repeated hook ids.
    /// - [`crate::error::EngineError::UnknownHookTarget`] for hook patches with
    ///   unknown ids.
    /// - [`crate::error::EngineError::Structured`] for foreign bases with gaps.
    /// - [`crate::error::EngineError::ChangedInRc`] for rc guards with
    ///   hooks-only changed leaves.
    /// - [`crate::error::EngineError::Read`] for unreadable files.
    /// - [`crate::error::EngineError::Utf8`] for non-UTF8 files.
    /// - [`crate::error::EngineError::Lua`] for Lua failures.
    pub fn evaluate(&self, profile: &Path) -> Result<Evaluated> {
        self.lua.set_app_data(self.stores.clone());
        let root = self.stores.resources().roots.project_root.clone();
        let project = Project::read(&root)?;
        self.lua.set_app_data(project.plugin_root(&root)?);
        install(&self.lua)?;
        let value = load_chunk(&self.lua, profile)?;
        let decoded = Profile::decode(&value, &self.lua)?;
        let assembler = Assembler::new(&self.lua);
        let documents = assembler.assemble(&decoded)?;
        let hooks = assembler.collect_hooks(&decoded, &documents)?;
        Ok(Evaluated { documents, hooks })
    }
}
