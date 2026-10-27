//! Patch tables under owner config names.

use crate::api::config::Config;
use crate::api::patch::{HookPatch, Patch, RcPatch, StructuredPatch};

/// One patch under its owner config name.
#[derive(Debug, Clone)]
pub struct OwnedPatch {
    /// The owner config name.
    pub owner: String,
    /// The ownerless patch.
    pub patch: Patch,
}

impl OwnedPatch {
    /// Build one owned patch.
    pub fn join_patch(patch: Patch, owner: &str) -> Self {
        Self {
            owner: owner.to_owned(),
            patch,
        }
    }

    /// Return the structured patch for structured payloads.
    pub fn structured(&self) -> Option<&StructuredPatch> {
        match &self.patch {
            Patch::Structured(found) => Some(found),
            Patch::Rc(_) | Patch::Hook(_) => None,
        }
    }

    /// Return the rc patch for rc sections.
    pub fn rc(&self) -> Option<&RcPatch> {
        match &self.patch {
            Patch::Rc(found) => Some(found),
            Patch::Structured(_) | Patch::Hook(_) => None,
        }
    }

    /// Return the hook patch for hook payloads.
    pub fn hook(&self) -> Option<&HookPatch> {
        match &self.patch {
            Patch::Hook(found) => Some(found),
            Patch::Structured(_) | Patch::Rc(_) => None,
        }
    }
}

/// Build owned patches from one config stamping the config name.
pub(crate) fn assemble_config(config: &Config) -> Vec<OwnedPatch> {
    config
        .patches
        .iter()
        .map(|patch| OwnedPatch::join_patch(patch.clone(), config.name.as_str()))
        .collect()
}
