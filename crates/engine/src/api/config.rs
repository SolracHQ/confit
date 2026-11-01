//! Plain config decode.

use std::collections::HashMap;

use lua_binding::Shape;

use super::patch::Patch;
use confit_model::document::Document;
use confit_model::hook::Hook;
use confit_model::id::Id;

/// One decoded config.
#[derive(Debug, Clone, Default, Shape)]
#[shape(table)]
pub struct Config {
    /// The config name.
    pub name: Id,
    /// Documents in declaration order with destination paths.
    #[shape(default)]
    pub documents: Vec<Document>,
    /// Patches in profile return order.
    #[shape(default)]
    pub patches: Vec<Patch>,
    /// Hooks in profile return order.
    #[shape(default)]
    pub hooks: Vec<Hook>,
    /// Requirements keyed by config name with hints.
    #[shape(default)]
    pub requirements: HashMap<String, String>,
}

impl Config {
    /// Build one config from its name.
    pub fn new(
        name: Id,
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
