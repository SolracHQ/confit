//! Assembles validated profiles.

mod collect;
mod freeze;
mod hooks;
mod merge;
pub(crate) mod owned;
mod patches;

use confit_core::error::TerminalResult;
use mlua::Lua;

use crate::api::profile::Profile;
use confit_model::document::Document;
use confit_model::hook::Hook;

pub struct Assembler<'a> {
    lua: &'a Lua,
}

impl<'a> Assembler<'a> {
    /// Builds one assembler.
    pub fn new(lua: &'a Lua) -> Self {
        Self { lua }
    }

    /// Returns the Lua instance.
    pub(super) fn lua(&self) -> &Lua {
        self.lua
    }

    /// Collects one profile into sorted documents.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when a
    ///   config name or a destination repeats.
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a
    ///   requirement is missing, a patch target is unknown, or a changed
    ///   condition targets an unbuilt route.
    /// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a
    ///   document fails to render.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback
    ///   fails or a shape fails to encode.
    pub fn assemble(&self, profile: &Profile) -> TerminalResult<Vec<Document>> {
        collect::validate_names(profile)?;
        collect::validate_requirements(profile)?;
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
        freeze::validate_rc_guards(&finished)?;
        Ok(finished)
    }

    /// Collects one profile's hooks.
    ///
    /// # Errors
    ///
    /// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a hook id
    ///   repeats, a patch target is unknown, or a changed condition targets an
    ///   unbuilt route.
    /// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a hook
    ///   fails to encode or a callback fails.
    pub fn collect_hooks(
        &self,
        profile: &Profile,
        documents: &[Document],
    ) -> TerminalResult<Vec<Hook>> {
        hooks::collect_hooks(self.lua, profile, documents)
    }
}

/// Collects one profile into sorted documents.
///
/// # Errors
///
/// - [`confit_core::error::kind::ErrorKind::DuplicateDocument`] when a
///   config name or a destination repeats.
/// - [`confit_core::error::kind::ErrorKind::Evaluate`] when a requirement is
///   missing, a patch target is unknown, or a changed condition targets an
///   unbuilt route.
/// - [`confit_core::error::kind::ErrorKind::RenderManifest`] when a document
///   fails to render.
/// - [`confit_core::error::kind::ErrorKind::LuaFailed`] when a callback fails
///   or a shape fails to encode.
pub fn assemble(lua: &Lua, profile: &Profile) -> TerminalResult<Vec<Document>> {
    Assembler::new(lua).assemble(profile)
}
