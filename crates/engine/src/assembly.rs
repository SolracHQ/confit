//! Assemble validated profiles into finished documents.

mod collect;
mod freeze;
mod hooks;
mod merge;
pub(crate) mod owned;
mod patches;

use confit_core::error::Outcome;
use mlua::Lua;

use crate::api::profile::Profile;
use confit_model::document::Document;
use confit_model::hook::Hook;

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
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated documents, missing
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for repeated documents, missing
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for repeated documents, missing
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for repeated documents, missing
    ///   requirements and targets, foreign bases, bad shapes, and Lua failures.
    pub fn assemble(&self, profile: &Profile) -> Outcome<Vec<Document>> {
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
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] for repeated ids, unknown targets,
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for repeated ids, unknown targets,
    ///   unbuilt routes, and Lua failures.
    pub fn collect_hooks(&self, profile: &Profile, documents: &[Document]) -> Outcome<Vec<Hook>> {
        hooks::collect_hooks(self.lua, profile, documents)
    }
}

/// Collect one profile into documents in destination order.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] for repeated documents, missing
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] for repeated documents, missing
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] for repeated documents, missing
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] for repeated documents, missing
///   requirements and targets, foreign bases, bad shapes, and Lua failures.
pub fn assemble(lua: &Lua, profile: &Profile) -> Outcome<Vec<Document>> {
    Assembler::new(lua).assemble(profile)
}
