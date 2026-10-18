//! Configs
//! Plain config decode into names with documents and patches.

use std::collections::HashMap;

use confit_binding_macros::Face;

use super::patch::Patch;
use confit_model::document::Document;
use confit_model::hook::Hook;

/// One decoded config.
#[derive(Debug, Clone, Default, Face)]
#[face(table)]
pub struct Config {
    /// The config name.
    pub name: String,
    /// Documents in declaration order with destination paths.
    #[face(default)]
    pub documents: Vec<Document>,
    /// Patches in profile return order.
    #[face(default)]
    pub patches: Vec<Patch>,
    /// Hooks in profile return order.
    #[face(default)]
    pub hooks: Vec<Hook>,
    /// Requirements keyed by config name with hints.
    #[face(default)]
    pub requirements: HashMap<String, String>,
}

impl Config {
    /// Build one config from its name with documents, patches, hooks, and requirements.
    pub fn new(
        name: String,
        documents: Vec<Document>,
        patches: Vec<Patch>,
        hooks: Vec<Hook>,
        requirements: HashMap<String, String>,
    ) -> Self {
        Self {
            name,
            documents,
            patches,
            hooks,
            requirements,
        }
    }
}
