//! Wraps patch tables with their owner.

use crate::api::config::Config;
use crate::api::patch::{HookPatch, Patch, RcPatch, StructuredPatch};

/// One patch and its owner config.
#[derive(Debug, Clone)]
pub struct OwnedPatch {
    /// The owner config name.
    pub owner: String,
    /// The ownerless patch.
    pub patch: Patch,
}

impl OwnedPatch {
    /// Builds one owned patch.
    pub fn join_patch(patch: Patch, owner: &str) -> Self {
        Self {
            owner: owner.to_owned(),
            patch,
        }
    }

    /// Returns the structured patch.
    pub fn structured(&self) -> Option<&StructuredPatch> {
        match &self.patch {
            Patch::Structured(found) => Some(found),
            Patch::Rc(_) | Patch::Hook(_) => None,
        }
    }

    /// Returns the rc patch.
    pub fn rc(&self) -> Option<&RcPatch> {
        match &self.patch {
            Patch::Rc(found) => Some(found),
            Patch::Structured(_) | Patch::Hook(_) => None,
        }
    }

    /// Returns the hook patch.
    pub fn hook(&self) -> Option<&HookPatch> {
        match &self.patch {
            Patch::Hook(found) => Some(found),
            Patch::Structured(_) | Patch::Rc(_) => None,
        }
    }
}

/// Wraps the config patches.
pub(crate) fn assemble_config(config: &Config) -> Vec<OwnedPatch> {
    config
        .patches
        .iter()
        .map(|patch| OwnedPatch::join_patch(patch.clone(), config.name.as_str()))
        .collect()
}
