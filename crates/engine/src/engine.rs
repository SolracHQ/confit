//! Evaluates profile files.

use std::path::Path;

use confit_core::error::{EvalError, TerminalResult, contextual, error, kind::ErrorKind};
use confit_core::version::VERSION;
use mlua::Lua;

use confit_events::EventHandles;
use confit_model::manifest::Manifest;
use confit_store::Stores;
use lua_binding::value::Decode;

use crate::api::profile::Profile;
use crate::assembly::Assembler;
use crate::loader::{install, load_chunk};
use crate::project::Project;

/// The profile evaluator.
pub struct Engine {
    /// The Lua instance.
    lua: Lua,
    /// The shared stores.
    stores: Stores,
    /// The renderer handles for Lua input.
    handles: EventHandles,
}

impl Engine {
    /// Builds one engine.
    pub fn new(stores: Stores, handles: EventHandles) -> Self {
        Self {
            lua: Lua::new(),
            stores,
            handles,
        }
    }

    /// Evaluates one profile file.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::OpenProfile`] when the
    ///   profile is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::LoadProject`] when the
    ///   project file is unreadable.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a profile
    ///   shape is bad, a requirement is missing, or a patch target is
    ///   unknown.
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when two
    ///   configs share a name or a destination.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a
    ///   document fails to render.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a chunk
    ///   fails to load or a callback fails.
    pub fn evaluate(&self, profile: &Path) -> TerminalResult<Manifest> {
        self.lua.set_app_data(self.stores.clone());
        self.lua.set_app_data(self.handles.clone());
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
        Ok(Manifest {
            version: VERSION,
            documents,
            hooks,
        })
    }
}
