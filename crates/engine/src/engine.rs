//! Evaluate Lua profiles into documents with hooks.

use std::path::Path;

use confit_core::error::{EvalError, TerminalResult, contextual, error, kind::ErrorKind};
use mlua::Lua;

use confit_binding::value::Decode;
use confit_model::document::Document;
use confit_model::hook::Hook;
use confit_store::Stores;

use crate::api::profile::Profile;
use crate::assembly::Assembler;
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
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] for unreadable profiles and projects,
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] for unreadable profiles and projects,
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for unreadable profiles and projects,
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for unreadable profiles and projects,
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for unreadable profiles and projects,
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for unreadable profiles and projects,
    ///   misshaped profiles, repeated documents, missing requirements and
    ///   targets, foreign bases, bad shapes, and Lua failures.
    pub fn evaluate(&self, profile: &Path) -> TerminalResult<Evaluated> {
        self.lua.set_app_data(self.stores.clone());
        let root = self.stores.resources().roots.project_root.clone();
        let project = Project::read(&root)?;
        self.lua.set_app_data(project.plugin_root(&root)?);
        install(&self.lua)?;
        let value = load_chunk(&self.lua, profile)?;
        let decoded = match Profile::decode(&value, &self.lua) {
            Ok(found) => found,
            Err(fault) => {
                return error(
                    ErrorKind::Evaluate,
                    contextual(EvalError::BadShape).with_detail(fault.to_string()),
                );
            }
        };
        let assembler = Assembler::new(&self.lua);
        let documents = assembler.assemble(&decoded)?;
        let hooks = assembler.collect_hooks(&decoded, &documents)?;
        Ok(Evaluated { documents, hooks })
    }
}
